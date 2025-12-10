# Builder
FROM rust:slim-bookworm AS builder

WORKDIR /app

# Системні залежності
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Встановлення цільової платформи WASM
RUN rustup target add wasm32-unknown-unknown

# Встановлення Dioxus CLI
RUN cargo install dioxus-cli --version 0.7.1

# Копіювання файлів конфігурації
COPY Cargo.toml Cargo.lock Dioxus.toml ./

# Копіювання файлів проекту
COPY src ./src
COPY assets ./assets
COPY migrations ./migrations
COPY gas_station.db ./gas_station.db

# Білдинг фронтенду
RUN dx build --release --features web --platform web

# Білдинг бекенду
RUN cargo build --release --features server --no-default-features --bin gas_station_experimental

# Runtime
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Встановлення бібліотек
RUN apt-get update && apt-get install -y \
    libssl3 \
    libsqlite3-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# БД
RUN mkdir -p /app/data
COPY --from=builder /app/gas_station.db /app/data/gas_station.db

# Копіювання бекенду
COPY --from=builder /app/target/release/gas_station_experimental /app/server

# Копіювання фронтенду
COPY --from=builder /app/target/dx/gas_station_experimental/release/web/public /app/public

# Копіювання assets
COPY --from=builder /app/assets /app/public/assets

# Налаштування змінних середовища
ENV PORT=8080
ENV IP=0.0.0.0
ENV DATABASE_URL=sqlite:///app/data/gas_station.db

# Робочий порт
EXPOSE 8080

CMD ["/app/server"]