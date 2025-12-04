# Builder
FROM rust:slim-bookworm AS builder

WORKDIR /app

# 1. Встановлюємо системні залежності
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# 2. Встановлюємо цільову платформу WASM
RUN rustup target add wasm32-unknown-unknown

# 3. Встановлюємо Dioxus CLI
RUN cargo install dioxus-cli --version 0.7.1

# 4. Копіюємо файли конфігурації
COPY Cargo.toml Cargo.lock Dioxus.toml ./

# 5. Копіюємо вихідний код
COPY src ./src
COPY assets ./assets
COPY migrations ./migrations

# 6a. Будуємо клієнтську частину (WASM)
RUN dx build --release --features web --platform web

# 6b. Будуємо серверну частину (Native binary)  
RUN cargo build --release --features server --no-default-features --bin gas_station_experimental

# Runtime
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# 1. Встановлюємо бібліотеки для запуску
RUN apt-get update && apt-get install -y \
    libssl3 \
    libsqlite3-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 2. Копіюємо бінарний файл
COPY --from=builder /app/target/release/gas_station_experimental /app/server

# 3. Копіюємо папку dist (фронтенд)
# Стандартний шлях виводу для dx build --platform web
COPY --from=builder /app/target/dx/gas_station_experimental/release/web/public /app/public

# 4. Копіюємо асети
COPY --from=builder /app/assets /app/public/assets

# 5. Налаштування змінних середовища
ENV PORT=8080
ENV IP=0.0.0.0
ENV DATABASE_URL=sqlite:///app/data/gas_station.db

# Відкриваємо порт
EXPOSE 8080

# Створюємо папку для даних
RUN mkdir -p /app/data

# 6. Запуск
CMD ["/app/server"]