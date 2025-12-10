#![allow(non_snake_case)]
use dioxus::prelude::*;
//use serde::{Deserialize, Serialize};

mod db;
mod models;
mod rate_limit;
mod schema;
mod utils;

#[cfg(feature = "server")]
use axum::{routing::get, Router};
#[cfg(feature = "server")]
use tower::ServiceBuilder;
#[cfg(feature = "server")]
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

use models::{Admin, Customer};

#[derive(Clone, Routable, Debug, PartialEq)]
enum Route {
    #[layout(Navbar)]
    #[route("/")]
    Home {},

    #[route("/login")]
    LoginPage {},

    #[route("/register")]
    RegisterPage {},

    #[route("/about")]
    About {},

    #[route("/prices")]
    PricesPage {},

    #[route("/admin/dashboard")]
    AdminDashboard {},

    #[route("/admin/management")]
    ManagementPage {},
}

fn main() {
    #[cfg(feature = "server")]
    {
        if let Err(e) = dotenvy::dotenv() {
            println!("LOG: .env not found: {}", e);
        }

        use std::path::Path;
        use std::fs;

        let db_path = "/app/data/gas_station.db";// Шлях на Volume
        let seed_path = "/station.db";// Шлях з Docker image

        if Path::new(seed_path).exists() {
            if !Path::new(db_path).exists() {
                println!("Init: База даних не знайдена на диску. Копіюємо шаблон...");
                match fs::copy(seed_path, db_path) {
                    Ok(_) => println!("Init: Шаблон успішно скопійовано!"),
                    Err(e) => println!("Init: Помилка копіювання шаблону: {}", e),
                }
            } else {
                println!("Init: База даних вже існує. Використовуємо поточні дані.");
            }
        } else {
            println!("Init: Шаблон бази (seed) не знайдено в образі.");
        }


        use axum::routing::get;
        use tower::ServiceBuilder;

        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let port = std::env::var("PORT").unwrap_or("8080".to_string());
                let addr = format!("0.0.0.0:{}", port);
                println!("Listening on {}", addr);
                let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

                let app = axum::Router::new()
                .serve_dioxus_application(ServeConfig::new(), App)
                .route("/api/test/read", axum::routing::get(test_api::test_read_fuels));

                axum::serve(listener, app).await.unwrap();
            });
    }

    launch(App);
}

fn App() -> Element {
    // Глобальний стан користувача
    use_context_provider(|| Signal::new(None::<Customer>));
    use_context_provider(|| Signal::new(None::<Admin>));

    rsx! {
        link { rel: "stylesheet", href: asset!("/assets/style.css") }
        Router::<Route> {}
    }
}

#[cfg(feature = "server")]
mod test_api {
    use super::*;
    use axum::response::IntoResponse;

    // Тестування отримання пального з БД
    pub async fn test_read_fuels() -> impl IntoResponse {
        match get_fuels_unblocked().await {
            Ok(list) => format!("OK: Found {} items", list.len()),
            Err(e) => format!("Error: {}", e),
        }
    }
}


#[server]
async fn register_user(login_str: String, pass_str: String) -> Result<(), ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::models::NewCustomer;
        use crate::schema::customer::dsl::*;
        use bcrypt::{hash, DEFAULT_COST};
        use diesel::prelude::*;

        let mut conn = db::connection();
        let count = customer
            .filter(login.eq(&login_str))
            .count()
            .get_result::<i64>(&mut conn)
            .unwrap_or(0);
        if count > 0 {
            return Err(ServerFnError::new("Такий логін вже існує"));
        }

        let pass_str_clone = pass_str.clone();
        let hashed_pass = tokio::task::spawn_blocking(move || hash(&pass_str_clone, DEFAULT_COST))
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        let new_user = NewCustomer {
            login: &login_str,
            password: &hashed_pass,
            salt: "bcrypt",
            balance: 100000,
        }; // Змінити сіль?

        diesel::insert_into(customer)
            .values(&new_user)
            .execute(&mut conn)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
    }
    Ok(())
}

#[server]
async fn login_user(login_str: String, pass_str: String) -> Result<Customer, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::rate_limit::check_rate_limit;
        use std::time::Duration;

        // Rate limiting: 5 спроб входу на хвилину (захист від brute-force)
        let rate_key = format!("login_{}", login_str);
        if !check_rate_limit(&rate_key, 20, Duration::from_secs(60)) {
            return Err(ServerFnError::new(
                "Забагато спроб входу. Спробуйте через хвилину.",
            ));
        }
        use crate::schema::customer::dsl::*;
        use bcrypt::verify;
        use diesel::prelude::*;

        let mut conn = db::connection();
        let user_data = customer
            .filter(login.eq(&login_str))
            .select((id, login, password, salt, balance))
            .first::<(i32, String, String, String, i64)>(&mut conn)
            .optional()
            .map_err(|e| ServerFnError::new(e.to_string()))?;

        if let Some((u_id, u_login, u_pass_hash, _, u_balance)) = user_data {
            // Password verification
            let pass_str_clone = pass_str.clone();
            let u_pass_hash_clone = u_pass_hash.clone();
            let is_valid = tokio::task::spawn_blocking(move || {
                verify(&pass_str_clone, &u_pass_hash_clone).unwrap_or(false)
            })
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;

            if is_valid {
                // Token generation
                let mut buf = [0u8; 32];
                getrandom::fill(&mut buf).unwrap_or(());
                let token: String = buf.iter().map(|b| format!("{:02x}", b)).collect();

                // Оновлення токену у БД
                diesel::update(customer.find(u_id))
                    .set(session_token.eq(&token))
                    .execute(&mut conn)
                    .map_err(|e| ServerFnError::new(e.to_string()))?;

                return Ok(Customer {
                    id: u_id,
                    login: u_login,
                    balance: u_balance,
                    session_token: Some(token),
                });
            }
        }
        return Err(ServerFnError::new("Хибні дані"));
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn login_admin(login_str: String, pass_str: String) -> Result<Admin, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::rate_limit::check_rate_limit;
        use std::time::Duration;

        // Rate limiting: 10 спроб входу на хвилину для адміністратора
        let rate_key = format!("admin_login_{}", login_str);
        if !check_rate_limit(&rate_key, 10, Duration::from_secs(60)) {
            return Err(ServerFnError::new(
                "Забагато спроб входу. Спробуйте через хвилину.",
            ));
        }
        use crate::schema::admin::dsl::*;
        use bcrypt::verify;
        use diesel::prelude::*;

        let mut conn = db::connection();
        let admin_data = admin
            .filter(login.eq(&login_str))
            .select((id, login, password))
            .first::<(i32, String, String)>(&mut conn)
            .optional()
            .map_err(|e| ServerFnError::new(e.to_string()))?;

        if let Some((a_id, a_login, a_pass_hash)) = admin_data {
            let pass_str_clone = pass_str.clone();
            let a_pass_hash_clone = a_pass_hash.clone();
            let is_valid = tokio::task::spawn_blocking(move || {
                verify(&pass_str_clone, &a_pass_hash_clone).unwrap_or(false)
            })
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;

            if is_valid {
                let mut buf = [0u8; 32];
                getrandom::fill(&mut buf).unwrap_or(());
                let token: String = buf.iter().map(|b| format!("{:02x}", b)).collect();

                // Оновлення токену у БД
                diesel::update(admin.find(a_id))
                    .set(session_token.eq(&token))
                    .execute(&mut conn)
                    .map_err(|e| ServerFnError::new(e.to_string()))?;

                return Ok(Admin {
                    id: a_id,
                    login: a_login,
                    session_token: Some(token),
                });
            }
        }
        return Err(ServerFnError::new("Хибні дані адміна"));
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn get_fuels() -> Result<Vec<models::FuelWithTank>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::rate_limit::check_rate_limit;
        use std::time::Duration;

        if !check_rate_limit("get_fuels_global", 60, Duration::from_secs(60)) {
            return Err(ServerFnError::new(
                "Занадто багато запитів. Спробуйте пізніше.",
            ));
        }
        use crate::models::{Fuel, FuelWithTank, Tank};
        use crate::schema::{fuel, tank};
        use diesel::prelude::*;
        use std::collections::HashMap;

        let mut conn = db::connection();

        let results: Vec<(Fuel, Tank)> = fuel::table
            .inner_join(tank::table)
            .load::<(Fuel, Tank)>(&mut conn)
            .map_err(|e| ServerFnError::new(e.to_string()))?;

        let mut grouped_fuels: HashMap<i32, FuelWithTank> = HashMap::new();

        for (f, t) in results {
            let f_type_str = f.fuel_type.unwrap_or_else(|| "petrol".to_string());

            grouped_fuels
                .entry(f.id)
                .and_modify(|item| {
                    item.stored += t.stored;
                    item.capacity += t.capacity;
                })
                .or_insert(FuelWithTank {
                    id: f.id,
                    name: f.name,
                    price: f.price,
                    fuel_type: f_type_str,
                    stored: t.stored,
                    capacity: t.capacity,
                });
        }

        let mut final_list: Vec<FuelWithTank> = grouped_fuels.into_values().collect();
        final_list.sort_by_key(|k| k.id);
        Ok(final_list)
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn get_fuels_unblocked() -> Result<Vec<models::FuelWithTank>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::rate_limit::check_rate_limit;
        use std::time::Duration;

        use crate::models::{Fuel, FuelWithTank, Tank};
        use crate::schema::{fuel, tank};
        use diesel::prelude::*;
        use std::collections::HashMap;

        let mut conn = db::connection();

        let results: Vec<(Fuel, Tank)> = fuel::table
            .inner_join(tank::table)
            .load::<(Fuel, Tank)>(&mut conn)
            .map_err(|e| ServerFnError::new(e.to_string()))?;

        let mut grouped_fuels: HashMap<i32, FuelWithTank> = HashMap::new();

        for (f, t) in results {
            let f_type_str = f.fuel_type.unwrap_or_else(|| "petrol".to_string());

            grouped_fuels
                .entry(f.id)
                .and_modify(|item| {
                    item.stored += t.stored;
                    item.capacity += t.capacity;
                })
                .or_insert(FuelWithTank {
                    id: f.id,
                    name: f.name,
                    price: f.price,
                    fuel_type: f_type_str,
                    stored: t.stored,
                    capacity: t.capacity,
                });
        }

        let mut final_list: Vec<FuelWithTank> = grouped_fuels.into_values().collect();
        final_list.sort_by_key(|k| k.id);
        Ok(final_list)
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn buy_fuel(
    user_id: i32,
    fuel_id: i32,
    amount_needed: i32,
    token_str: String,
) -> Result<i64, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::models::{Bank, Tank};
        use crate::schema::{bank, customer, fuel, tank};
        use diesel::prelude::*;

        let mut conn = db::connection();

        // Перевірка токену
        let stored_token: Option<String> = customer::table
            .find(user_id)
            .select(customer::session_token)
            .first(&mut conn)
            .map_err(|_| ServerFnError::new("User not found"))?;

        if stored_token.is_none() || stored_token.unwrap() != token_str {
            return Err(ServerFnError::new("Неавторизований доступ (Invalid Token)"));
        }

        // Перевірка доступності пального перед транзакцією
        let (fuel_price, f_type_opt): (i64, Option<String>) = fuel::table
            .find(fuel_id)
            .select((fuel::price, fuel::fuel_type))
            .first(&mut conn)
            .map_err(|e| ServerFnError::new(format!("Паливо не знайдено: {}", e)))?;

        let f_type = f_type_opt.as_deref().unwrap_or("petrol");
        let is_electricity = f_type == "electricity";

        let all_tanks: Vec<Tank> = tank::table
            .filter(tank::fuelid.eq(fuel_id))
            .load::<Tank>(&mut conn)
            .map_err(|e| ServerFnError::new(format!("Помилка завантаження резервуарів: {}", e)))?;

        if is_electricity {
            let any_working = all_tanks.iter().any(|t| t.stored > 0);
            if !any_working {
                return Err(ServerFnError::new("Зарядка тимчасово недоступна"));
            }
        } else {
            let total_stored: i32 = all_tanks.iter().map(|t| t.stored).sum();
            if total_stored < amount_needed {
                return Err(ServerFnError::new("Недостатньо пального на складі"));
            }
        }

        let total_cost = fuel_price * amount_needed as i64;
        let current_balance: i64 = customer::table
            .find(user_id)
            .select(customer::balance)
            .first(&mut conn)
            .map_err(|e| ServerFnError::new(format!("Помилка отримання балансу: {}", e)))?;

        if current_balance < total_cost {
            return Err(ServerFnError::new("Недостатньо коштів на балансі"));
        }

        // Транзакція
        conn.transaction::<i64, diesel::result::Error, _>(|conn| {
            if !is_electricity {
                let mut remaining_to_take = amount_needed;
                for t in all_tanks {
                    if remaining_to_take <= 0 {
                        break;
                    }
                    if t.stored <= 0 {
                        continue;
                    }

                    let take = std::cmp::min(remaining_to_take, t.stored);
                    diesel::update(tank::table.find(t.id))
                        .set(tank::stored.eq(t.stored - take))
                        .execute(conn)?;

                    remaining_to_take -= take;
                }
            }

            diesel::update(customer::table.find(user_id))
                .set(customer::balance.eq(current_balance - total_cost))
                .execute(conn)?;

            let bank_row = bank::table.first::<Bank>(conn).optional()?;
            if let Some(b) = bank_row {
                diesel::update(bank::table.find(b.id))
                    .set(bank::total.eq(b.total + total_cost))
                    .execute(conn)?;
            } else {
                diesel::insert_into(bank::table)
                    .values(bank::total.eq(total_cost))
                    .execute(conn)?;
            }

            Ok(current_balance - total_cost)
        })
        .map_err(|e| ServerFnError::new(format!("Помилка транзакції: {}", e)))
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn buy_fuel_batch(
    user_id: i32,
    items: Vec<(i32, i32)>, // (fuel_id, amount)
    token_str: String,
) -> Result<i64, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::models::{Bank, Tank};
        use crate::schema::{bank, customer, fuel, tank};
        use diesel::prelude::*;

        let mut conn = db::connection();

        // Перевірка токену
        let stored_token: Option<String> = customer::table
            .find(user_id)
            .select(customer::session_token)
            .first(&mut conn)
            .map_err(|_| ServerFnError::new("User not found"))?;

        if stored_token.is_none() || stored_token.unwrap() != token_str {
            return Err(ServerFnError::new("Неавторизований доступ (Invalid Token)"));
        }

        if items.is_empty() {
            return Err(ServerFnError::new("Кошик пустий"));
        }

        // Перевірка доступності пального та розрахунок вартості
        let mut total_cost: i64 = 0;
        let mut updates = Vec::new();

        for (f_id, amount) in &items {
            let (fuel_price, f_type_opt): (i64, Option<String>) = fuel::table
                .find(f_id)
                .select((fuel::price, fuel::fuel_type))
                .first(&mut conn)
                .map_err(|e| ServerFnError::new(format!("Паливо не знайдено: {}", e)))?;

            let f_type = f_type_opt.as_deref().unwrap_or("petrol");
            let is_electricity = f_type == "electricity";
            let cost = fuel_price * (*amount as i64);
            total_cost += cost;

            let all_tanks: Vec<Tank> = tank::table
                .filter(tank::fuelid.eq(f_id))
                .load::<Tank>(&mut conn)
                .map_err(|e| ServerFnError::new(format!("Помилка завантаження резервуарів: {}", e)))?;

            if is_electricity {
                let any_working = all_tanks.iter().any(|t| t.stored > 0);
                if !any_working {
                    return Err(ServerFnError::new("Зарядка тимчасово недоступна"));
                }
            } else {
                let total_stored: i32 = all_tanks.iter().map(|t| t.stored).sum();
                if total_stored < *amount {
                    return Err(ServerFnError::new("Недостатньо пального на складі"));
                }
            }

            updates.push((*f_id, *amount, is_electricity, all_tanks));
        }

        // Перевірка балансу
        let current_balance: i64 = customer::table
            .find(user_id)
            .select(customer::balance)
            .first(&mut conn)
            .map_err(|e| ServerFnError::new(format!("Помилка отримання балансу: {}", e)))?;

        if current_balance < total_cost {
            return Err(ServerFnError::new("Недостатньо коштів на балансі"));
        }

        // Виконуємо транзакцію
        conn.transaction::<i64, diesel::result::Error, _>(|conn| {

            for (_f_id, amount, is_electricity, all_tanks) in updates {
                if !is_electricity {
                    let mut remaining_to_take = amount;
                    for t in all_tanks {
                        if remaining_to_take <= 0 {
                            break;
                        }
                        if t.stored <= 0 {
                            continue;
                        }

                        let take = std::cmp::min(remaining_to_take, t.stored);
                        diesel::update(tank::table.find(t.id))
                            .set(tank::stored.eq(t.stored - take))
                            .execute(conn)?;

                        remaining_to_take -= take;
                    }
                }
            }

            //Update user balance
            diesel::update(customer::table.find(user_id))
                .set(customer::balance.eq(current_balance - total_cost))
                .execute(conn)?;

            //Update bank
            let bank_row = bank::table.first::<Bank>(conn).optional()?;
            if let Some(b) = bank_row {
                diesel::update(bank::table.find(b.id))
                    .set(bank::total.eq(b.total + total_cost))
                    .execute(conn)?;
            } else {
                diesel::insert_into(bank::table)
                    .values(bank::total.eq(total_cost))
                    .execute(conn)?;
            }

            Ok(current_balance - total_cost)
        })
        .map_err(|e| ServerFnError::new(format!("Помилка транзакції: {}", e)))
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn update_fuel_price(
    fuel_id: i32,
    new_price: i64,
    token_str: String,
) -> Result<(), ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::schema::{admin, fuel};
        use diesel::prelude::*;

        let mut conn = db::connection();

        // Перевірка токену адміна
        let count = admin::table
            .filter(admin::session_token.eq(&token_str))
            .count()
            .get_result::<i64>(&mut conn)
            .unwrap_or(0);

        if count == 0 {
            return Err(ServerFnError::new("Unauthorized Admin"));
        }

        diesel::update(fuel::table.find(fuel_id))
            .set(fuel::price.eq(new_price))
            .execute(&mut conn)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        Ok(())
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn refill_fuel(fuel_id: i32, amount: i32, token_str: String) -> Result<(), ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::models::{Bank, Tank};
        use crate::schema::{admin, bank, fuel, tank};
        use diesel::prelude::*;

        let mut conn = db::connection();

        // Перевірка токену адміна
        let count = admin::table
            .filter(admin::session_token.eq(&token_str))
            .count()
            .get_result::<i64>(&mut conn)
            .unwrap_or(0);

        if count == 0 {
            return Err(ServerFnError::new("Unauthorized Admin"));
        }

        // Перевірка ціни та розрахунок вартості
        let price: i64 = fuel::table
            .find(fuel_id)
            .select(fuel::price)
            .first(&mut conn)
            .map_err(|e| ServerFnError::new(format!("Паливо не знайдено: {}", e)))?;
        
        let cost_per_unit = price / 2;
        let total_cost = cost_per_unit * amount as i64;

        // Перевірка балансу банку
        let bank_row = bank::table
            .first::<Bank>(&mut conn)
            .optional()
            .map_err(|e| ServerFnError::new(format!("Помилка отримання балансу банку: {}", e)))?;
        
        let current_bank_total = bank_row.as_ref().map(|b| b.total).unwrap_or(0);

        if current_bank_total < total_cost {
            return Err(ServerFnError::new("Недостатньо коштів у банку"));
        }

        if bank_row.is_none() {
            return Err(ServerFnError::new("Банк не ініціалізовано"));
        }

        // Перевірка наявності місця в резервуарах
        let tanks: Vec<Tank> = tank::table
            .filter(tank::fuelid.eq(fuel_id))
            .load::<Tank>(&mut conn)
            .map_err(|e| ServerFnError::new(format!("Помилка завантаження резервуарів: {}", e)))?;

        let total_space: i32 = tanks.iter().map(|t| t.capacity - t.stored).sum();
        if total_space < amount {
            return Err(ServerFnError::new("Недостатньо місця в резервуарах"));
        }

        // Виконуємо транзакцію
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            if let Some(b) = bank_row {
                diesel::update(bank::table.find(b.id))
                    .set(bank::total.eq(b.total - total_cost))
                    .execute(conn)?;
            }

            let mut remaining = amount;
            for t in tanks {
                if remaining <= 0 {
                    break;
                }
                let space = t.capacity - t.stored;
                if space > 0 {
                    let add = std::cmp::min(remaining, space);
                    diesel::update(tank::table.find(t.id))
                        .set(tank::stored.eq(t.stored + add))
                        .execute(conn)?;
                    remaining -= add;
                }
            }

            Ok(())
        })
        .map_err(|e| ServerFnError::new(format!("Помилка транзакції: {}", e)))
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn get_bank_info() -> Result<models::Bank, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use crate::models::Bank;
        use crate::schema::bank;
        use diesel::prelude::*;

        let mut conn = db::connection();
        let bank_row = bank::table
            .first::<Bank>(&mut conn)
            .optional()
            .map_err(|e| ServerFnError::new(e.to_string()))?;

        match bank_row {
            Some(b) => Ok(b),
            None => Ok(Bank { id: 0, total: 0 }),
        }
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[server]
async fn fetch_fuel_prices() -> Result<Vec<models::FuelPriceStats>, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use models::FuelPriceStats;
        use reqwest::get;
        use scraper::{Html, Selector};

        let url = "https://index.minfin.com.ua/ua/markets/fuel/tm/";

        let response = get(url)
            .await
            .map_err(|e| ServerFnError::new(format!("Помилка завантаження: {}", e)))?;
        let html_content = response
            .text()
            .await
            .map_err(|e| ServerFnError::new(format!("Помилка читання: {}", e)))?;
        let document = Html::parse_document(&html_content);

        let row_selector = Selector::parse("table tr")
            .map_err(|e| ServerFnError::new(format!("Помилка селектора: {}", e)))?;
        let cell_selector = Selector::parse("td")
            .map_err(|e| ServerFnError::new(format!("Помилка селектора: {}", e)))?;

        let mut a95_premium = Vec::new();
        let mut a95 = Vec::new();
        let mut a92 = Vec::new();
        let mut diesel = Vec::new();
        let mut gas = Vec::new();

        for row in document.select(&row_selector) {
            let cells: Vec<String> = row
                .select(&cell_selector)
                .map(|c| c.text().collect::<Vec<_>>().join("").trim().to_string())
                .collect();

            if cells.len() >= 6 && !cells[0].is_empty() {
                if let Some(price) = parse_price(&cells[1]) {
                    a95_premium.push(price);
                }
                if let Some(price) = parse_price(&cells[2]) {
                    a95.push(price);
                }
                if let Some(price) = parse_price(&cells[3]) {
                    a92.push(price);
                }
                if let Some(price) = parse_price(&cells[4]) {
                    diesel.push(price);
                }
                if let Some(price) = parse_price(&cells[5]) {
                    gas.push(price);
                }
            }
        }

        let mut results = Vec::new();

        if !a95_premium.is_empty() {
            results.push(calculate_stats("А-95 Преміум", &a95_premium));
        }
        if !a95.is_empty() {
            results.push(calculate_stats("А-95", &a95));
        }
        if !a92.is_empty() {
            results.push(calculate_stats("А-92", &a92));
        }
        if !diesel.is_empty() {
            results.push(calculate_stats("Дизель", &diesel));
        }
        if !gas.is_empty() {
            results.push(calculate_stats("Газ", &gas));
        }

        Ok(results)
    }
    #[cfg(not(feature = "server"))]
    Err(ServerFnError::new("Server only"))
}

#[cfg(feature = "server")]
fn parse_price(raw: &str) -> Option<f64> {
    if raw.is_empty() || raw == "-" {
        return None;
    }
    raw.replace(",", ".").parse::<f64>().ok()
}

#[cfg(feature = "server")]
fn calculate_stats(name: &str, prices: &[f64]) -> models::FuelPriceStats {
    let sum: f64 = prices.iter().sum();
    let count = prices.len();
    let avg = sum / count as f64;
    let min = prices.iter().fold(f64::INFINITY, |a, &b| a.min(b));
    let max = prices.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

    models::FuelPriceStats {
        name: name.to_string(),
        average: avg,
        min,
        max,
        count,
    }
}

#[component]
fn Navbar() -> Element {
    let mut user_state = use_context::<Signal<Option<Customer>>>();
    let mut admin_state = use_context::<Signal<Option<Admin>>>();
    let nav = use_navigator();
    let fmt_price = |cents: i64| format!("{:.2} грн", cents as f64 / 100.0);

    let handle_logout = move |_| {
        user_state.set(None);
        admin_state.set(None);
        nav.push(Route::LoginPage {});
    };

    rsx! {
        nav { class: "navbar",
            div { class: "nav-brand",
                Link { to: Route::Home {}, "GasStation" }
            }
            div { class: "nav-links",
                Link { to: Route::Home {}, class: "nav-item", "Головна" }
                Link { to: Route::PricesPage {}, class: "nav-item", "Ціни" }

                if let Some(user) = user_state() {
                    div { class: "user-badge",
                        span { class: "user-name", "{user.login}" }
                        span { class: "user-balance", "{fmt_price(user.balance)}" }
                    }
                    button { class: "nav-item logout-btn", onclick: handle_logout, "Вийти" }
                } else if let Some(admin) = admin_state() {
                    div { class: "user-badge",
                        style: "background-color: #4309a7ff; color: white;",
                        span { class: "user-name", "Admin: {admin.login}" }
                    }
                    //Link { to: Route::AdminDashboard {}, class: "nav-item", "Дашборд" }
                    Link { to: Route::ManagementPage {}, class: "nav-item", "Керування" }
                    button { class: "nav-item logout-btn", onclick: handle_logout, "Вийти" }
                } else {
                    Link { to: Route::LoginPage {}, class: "nav-item", "Вхід" }
                    Link { to: Route::RegisterPage {}, class: "nav-item highlight", "Реєстрація" }
                }
            }
        }
        Outlet::<Route> {}
    }
}

// Helper function to clean error messages
fn clean_error_msg(error: String) -> String {
    error
        .replace("error running server function: ", "")
        .replace(" (details: None)", "")
}

#[component]
fn FuelCard(props: models::FuelCardProps) -> Element {
    let item = props.item;
    let mut cart = props.cart;
    let mut amount = use_signal(|| 1);
    let mut user_state = use_context::<Signal<Option<Customer>>>();
    let mut error_msg = use_signal(|| "".to_string());
    let nav = use_navigator();

    let is_electric = item.fuel_type == "electricity";

    let icon = match item.fuel_type.as_str() {
        "electricity" => "⚡",
        "gas" => "☁️",
        "diesel" => "🛢️",
        _ => "⛽",
    };

    let unit = if is_electric { "кВт год" } else { "л" };

    let price_val = item.price;
    let total_cost = price_val * amount() as i64;
    let fmt_money = |cents: i64| format!("{:.2}", cents as f64 / 100.0);

    // progress bar logic
    let percentage = if !is_electric && item.capacity > 0 {
        (item.stored as f64 / item.capacity as f64) * 100.0
    } else {
        0.0
    };
    let bar_color = if percentage < 20.0 {
        "#ef4444"
    } else {
        "#059669"
    };

    let is_available = if is_electric {
        item.stored > 0
    } else {
        item.stored >= 1
    };

    let handle_buy = move |_| async move {
        if let Some(user) = user_state() {
            error_msg.set("Processing".to_string());
            let token = user.session_token.clone().unwrap_or_default();
            match buy_fuel(user.id, item.id, amount(), token).await {
                Ok(new_balance) => {
                    let mut updated_user = user.clone();
                    updated_user.balance = new_balance;
                    user_state.set(Some(updated_user));
                    amount.set(1);
                    error_msg.set("Успішно!".to_string());
                }
                Err(e) => error_msg.set(clean_error_msg(e.to_string())),
                // Err(e) => error_msg.set(e.to_string()),
            }
        } else {
            nav.push(Route::LoginPage {});
        }
    };

    let is_selected = cart().contains_key(&item.id);
    let handle_toggle_cart = move |_| {
        if cart().contains_key(&item.id) {
            cart.write().remove(&item.id);
        } else {
            cart.write().insert(item.id, amount());
        }
    };

    rsx! {
        div { class: "fuel-item",
            div { class: "fuel-header",
                div { class: "fuel-icon", "{icon}" }
                div {
                    span { class: "fuel-name", "{item.name}" }
                    div {  }
                    span { class: "fuel-price", "{fmt_money(price_val)} грн/{unit}" }
                }
                if user_state().is_some() && is_available {
                    div { style: "margin-left: auto;",
                        input {
                            type: "checkbox",
                            checked: "{is_selected}",
                            onchange: handle_toggle_cart,
                            style: "transform: scale(1.5); cursor: pointer;"
                        }
                    }
                }
            }

            if !is_electric {
                div {
                    style: "padding: 10px 20px 0 20px; display: flex; flex-direction: column; gap: 5px;",
                    div {
                        style: "width: 100%; height: 10px; background-color: #e5e7eb; border-radius: 5px; overflow: hidden;",
                        div { style: "height: 100%; width: {percentage}%; background-color: {bar_color}; transition: width 0.5s ease;" }
                    }
                }
            } else {
                div {
                    style: "padding: 10px 20px 0 20px; font-size: 0.9rem; font-weight: bold;",
                    if is_available {
                        span { style: "color: #059669;", "🟢 Зарядка доступна" }
                    } else {
                        span { style: "color: #dc2626;", "🔴 Тимчасово недоступно" }
                    }
                }
            }

            div { class: "fuel-controls",
                div { class: "slider-container",
                    label { "Купити: {amount} {unit}" }
                    input {
                        type: "range",
                        min: "1",
                        max: if is_electric { "100" } else { "{item.stored}" },
                        value: "{amount}",
                        oninput: move |e| {
                            let val = e.value().parse().unwrap_or(1);
                            amount.set(val);
                            if cart().contains_key(&item.id) {
                                cart.write().insert(item.id, val);
                            }
                        }
                    }
                }

                div { class: "total-price",
                    "До сплати: "
                    span { class: "price-tag", "{fmt_money(total_cost)} грн" }
                }

                if !error_msg().is_empty() {
                     div { class: "mini-error", "{error_msg}" }
                }

                button {
                    class: "buy-button",
                    disabled: !is_available,
                    style: if !is_available { "background-color: gray; cursor: not-allowed;" } else { "" },
                    onclick: handle_buy,
                    if !is_available { "Недоступно" } else { "Купити" }
                }
            }
        }
    }
}

#[component]
fn About() -> Element {
    const ABOUT_HTML: &str = include_str!("../assets/about.html");
    rsx! {
        div {
            dangerous_inner_html: "{ABOUT_HTML}"
        }
    }
}

#[component]
fn PricesPage() -> Element {
    let prices = use_resource(|| fetch_fuel_prices());

    rsx! {
        div { class: "page-container",
            div { class: "content-card",
                h1 { "Середні ціни на пальне в Україні" }
                p { class: "subtitle", style: "margin-bottom: 2rem;",
                    "Дані оновлюються згідно з даними Мінфін"
                }

                match &*prices.read() {
                    Some(Ok(list)) => rsx! {
                        div { style: "overflow-x: auto;",
                            table { style: "width: 100%; border-collapse: collapse;",
                                thead {
                                    tr { style: "background-color: #f3f4f6;",
                                        th { style: "padding: 12px; text-align: left; border-bottom: 2px solid #e5e7eb;", "Тип пального" }
                                        th { style: "padding: 12px; text-align: right; border-bottom: 2px solid #e5e7eb;", "Середня ціна" }
                                        th { style: "padding: 12px; text-align: right; border-bottom: 2px solid #e5e7eb;", "Мін" }
                                        th { style: "padding: 12px; text-align: right; border-bottom: 2px solid #e5e7eb;", "Макс" }
                                    }
                                }
                                tbody {
                                    for item in list {
                                        tr { key: "{item.name}", style: "border-bottom: 1px solid #e5e7eb;",
                                            td { style: "padding: 12px; font-weight: 600;", "{item.name}" }
                                            td { style: "padding: 12px; text-align: right; color: #2563eb; font-weight: bold;",
                                                "{item.average:.2} грн"
                                            }
                                            td { style: "padding: 12px; text-align: right; color: #6b7280;",
                                                "{item.min:.2}"
                                            }
                                            td { style: "padding: 12px; text-align: right; color: #6b7280;",
                                                "{item.max:.2}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Some(Err(e)) => rsx! {
                        div { class: "error-message",
                            "Помилка завантаження даних: {e}"
                        }
                    },
                    None => rsx! {
                        div { class: "loading",
                            "Завантаження цін"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Footer() -> Element {
    rsx! {
        footer {
            style: "background-color: #1f2937; color: white; padding: 2rem; margin-top: auto; width: 100%;",
            div { style: "max-width: 1200px; margin: 0 auto; display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 20px;",
                div {
                    h3 { style: "margin: 0;", "GasStation" }
                    p { style: "font-size: 0.9rem; color: #9ca3af; margin: 5px 0 0 0;", "© 2025 Всі права захищені" }
                }
                div { style: "display: flex; gap: 20px;",
                    a { href: "/about", style: "color: white; text-decoration: none;", "Про компанію" }
                }
            }
        }
    }
}

#[component]
fn Home() -> Element {
    let mut fuels = use_resource(|| get_fuels());
    let mut user_state = use_context::<Signal<Option<Customer>>>();
    let mut cart = use_signal(|| std::collections::HashMap::<i32, i32>::new());
    let mut error_msg = use_signal(|| "".to_string());
    let nav = use_navigator();

    let handle_buy_batch = move |_| async move {
        if let Some(user) = user_state() {
            error_msg.set("Processing...".to_string());
            let token = user.session_token.clone().unwrap_or_default();
            let items: Vec<(i32, i32)> = cart().into_iter().collect();

            if items.is_empty() {
                error_msg.set("Кошик пустий".to_string());
                return;
            }

            match buy_fuel_batch(user.id, items, token).await {
                Ok(new_balance) => {
                    let mut updated_user = user.clone();
                    updated_user.balance = new_balance;
                    user_state.set(Some(updated_user));
                    cart.write().clear();
                    error_msg.set("Успішно куплено!".to_string());
                    fuels.restart(); // Refresh fuel data
                }
                Err(e) => error_msg.set(clean_error_msg(e.to_string())),
            }
        } else {
            nav.push(Route::LoginPage {});
        }
    };

    let total_cart_cost = {
        let current_cart = cart();
        if let Some(Ok(list)) = &*fuels.read() {
            list.iter()
                .filter_map(|f| {
                    current_cart
                        .get(&f.id)
                        .map(|&amount| f.price * amount as i64)
                })
                .sum::<i64>()
        } else {
            0
        }
    };
    let fmt_money = |cents: i64| format!("{:.2}", cents as f64 / 100.0);

    rsx! {
        div { style: "display: flex; flex-direction: column; min-height: 100vh;",
            div { class: "page-container", style: "flex: 1; flex-direction: column; align-items: center; gap: 2rem;",
                div { class: "content-card",
                    if user_state().is_none() {
                        h1 { "Асортимент" }
                        p { class: "subtitle", "Увійдіть, щоб купити пальне" }
                    } else {
                        h1 { "Оберіть пальне" }
                    }

                    match &*fuels.read() {
                        Some(Ok(list)) => rsx! {
                            div { class: "fuel-grid",
                                for item in list {
                                    FuelCard {
                                        key: "{item.id}",
                                        item: item.clone(),
                                        cart: cart
                                    }
                                }
                            }
                        },
                        Some(Err(e)) => rsx! { div { class: "error-message", "Помилка: {e}" } },
                        None => rsx! { div { class: "loading", "Завантаження" } }
                    }

                    if !cart().is_empty() && user_state().is_some() {
                        div { class: "batch-controls", style: "margin-top: 20px; padding: 20px; border-top: 1px solid #e5e7eb; display: flex; flex-direction: column; align-items: center; gap: 10px;",
                            div { style: "font-size: 1.2rem; font-weight: bold;",
                                "Разом до сплати: "
                                span { style: "color: #2563eb;", "{fmt_money(total_cart_cost)} грн" }
                            }
                            if !error_msg().is_empty() {
                                div { class: "mini-error", "{error_msg}" }
                            }
                            button {
                                class: "modern-button",
                                style: "width: 100%; max-width: 300px; background-color: #2563eb;",
                                onclick: handle_buy_batch,
                                "Купити вибране"
                            }
                        }
                    }
                }
            }
            Footer {}
        }
    }
}

#[component]
fn LoginPage() -> Element {
    let mut login = use_signal(|| "".to_string());
    let mut password = use_signal(|| "".to_string());
    let mut error_msg = use_signal(|| "".to_string());

    let mut user_state = use_context::<Signal<Option<Customer>>>();
    let mut admin_state = use_context::<Signal<Option<Admin>>>();
    let nav = use_navigator();

    let handle_login = move |_| async move {
        error_msg.set("".to_string());
        let l = login();
        let p = password();

        // Try user login first
        match login_user(l.clone(), p.clone()).await {
            Ok(user) => {
                user_state.set(Some(user));
                nav.push(Route::Home {});
            }
            Err(_) => {
                // Try admin login
                match login_admin(l, p).await {
                    Ok(admin) => {
                        admin_state.set(Some(admin));
                        nav.push(Route::ManagementPage {});
                    }
                    Err(_) => error_msg.set("Невірний логін або пароль".to_string()),
                }
            }
        }
    };

    rsx! {
        div { class: "page-container",
            div { class: "auth-card",
                h2 { "Вхід в кабінет" }
                div { class: "form-content",
                    input { class: "modern-input", placeholder: "Логін", value: "{login}", oninput: move |e| login.set(e.value()) }
                    input { class: "modern-input", type: "password", placeholder: "Пароль", value: "{password}", oninput: move |e| password.set(e.value()) }
                    if !error_msg().is_empty() { div { class: "error-message", "{error_msg}" } }
                    button { class: "modern-button", onclick: handle_login, "Увійти" }
                }
            }
        }
    }
}

#[component]
fn RegisterPage() -> Element {
    let mut login = use_signal(|| "".to_string());
    let mut password = use_signal(|| "".to_string());
    let mut error_msg = use_signal(|| "".to_string());
    let nav = use_navigator();

    let handle_register = move |_| async move {
        error_msg.set("".to_string());
        match register_user(login(), password()).await {
            Ok(_) => {
                nav.push(Route::LoginPage {});
            }
            Err(e) => error_msg.set(e.to_string()),
        }
    };

    rsx! {
        div { class: "page-container",
            div { class: "auth-card",
                h2 { "Створити акаунт" }
                div { class: "form-content",
                    input { class: "modern-input", placeholder: "Логін", value: "{login}", oninput: move |e| login.set(e.value()) }
                    input { class: "modern-input", type: "password", placeholder: "Пароль", value: "{password}", oninput: move |e| password.set(e.value()) }
                    if !error_msg().is_empty() { div { class: "error-message", "{error_msg}" } }
                    button { class: "modern-button", onclick: handle_register, "Зареєструватися" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[component]
fn AdminDashboard() -> Element {
    let admin_state = use_context::<Signal<Option<Admin>>>();
    let nav = use_navigator();

    if admin_state().is_none() {
        nav.push(Route::LoginPage {});
        return rsx! {};
    }

    rsx! {
        div { class: "page-container",
            h1 { "Панель адміністратора" }
            div { class: "admin-header",
                p { "Вітаємо, {admin_state().unwrap().login}!" }
            }
            div { class: "content-card",
                p { "Тут поки що пусто. Перейдіть на вкладку 'Керування'." }
            }
        }
    }
}

#[component]
fn ManagementPage() -> Element {
    let admin_state = use_context::<Signal<Option<Admin>>>();
    let nav = use_navigator();
    let fuels = use_resource(|| get_fuels());
    let bank_info = use_resource(|| get_bank_info());

    if admin_state().is_none() {
        nav.push(Route::LoginPage {});
        return rsx! {};
    }

    let fmt_money = |cents: i64| format!("{:.2} грн", cents as f64 / 100.0);

    rsx! {
        div { class: "page-container",
            div { class: "management-layout",
                // Left Column: Header and Bank Info
                div { class: "management-sidebar",
                    h1 { style: "text-align: left;", "Керування заправкою" }
                    div { class: "admin-header",
                        match &*bank_info.read() {
                            Some(Ok(bank)) => rsx! {
                                div { class: "bank-info",
                                    span { "Баланс банку: " }
                                    span { class: "money-value", "{fmt_money(bank.total)}" }
                                }
                            },
                            Some(Err(e)) => rsx! { div { class: "error", "Помилка банку: {e}" } },
                            None => rsx! { "Завантаження..." }
                        }
                    }
                }

                // Right Column: Fuel Management
                div { class: "content-card management-content",
                    h2 { "Керування пальним" }
                    match &*fuels.read() {
                        Some(Ok(list)) => rsx! {
                            div { class: "fuel-grid",
                                for item in list {
                                    AdminFuelItem { key: "{item.id}", item: item.clone(), bank_info: bank_info, fuels: fuels }
                                }
                            }
                        },
                        Some(Err(e)) => rsx! { div { class: "error-message", "Помилка: {e}" } },
                        None => rsx! { div { class: "loading", "Завантаження" } }
                    }
                }
            }
        }
    }
}

#[component]
fn AdminFuelItem(
    item: models::FuelWithTank,
    bank_info: Resource<Result<models::Bank, ServerFnError>>,
    fuels: Resource<Result<Vec<models::FuelWithTank>, ServerFnError>>,
) -> Element {
    let mut price_input = use_signal(|| (item.price as f64 / 100.0).to_string());
    let mut refill_amount = use_signal(|| 100);
    let mut msg = use_signal(|| "".to_string());
    let admin_state = use_context::<Signal<Option<Admin>>>();

    let is_electric = item.fuel_type == "electricity";
    let unit = if is_electric { "кВт год" } else { "л" };

    let handle_save_price = move |_| async move {
        msg.set("Saving...".to_string());
        if let Some(admin) = admin_state() {
            let token = admin.session_token.clone().unwrap_or_default();
            if let Ok(val) = price_input().parse::<f64>() {
                let cents = (val * 100.0) as i64;
                match update_fuel_price(item.id, cents, token).await {
                    Ok(_) => msg.set("Ціна оновлена".to_string()),
                    Err(e) => msg.set(clean_error_msg(e.to_string())),
                    // Err(e) => msg.set(e.to_string()),
                }
            } else {
                msg.set("Невірний формат".to_string());
            }
        } else {
            msg.set("Not logged in".to_string());
        }
    };

    let handle_refill = move |_| async move {
        msg.set("Refilling".to_string());
        if let Some(admin) = admin_state() {
            let token = admin.session_token.clone().unwrap_or_default();
            match refill_fuel(item.id, refill_amount(), token).await {
                Ok(_) => {
                    msg.set("Заправлено!".to_string());
                    bank_info.restart(); // Оновити баланс банку
                    fuels.restart(); // Оновити дані про пальне
                }
                Err(e) => msg.set(clean_error_msg(e.to_string())),
                // Err(e) => msg.set(e.to_string()),
            }
        } else {
            msg.set("Not logged in".to_string());
        }
    };

    let refill_cost = (item.price / 2) * refill_amount() as i64;
    let fmt_money = |cents: i64| format!("{:.2}", cents as f64 / 100.0);

    rsx! {
        div { class: "fuel-item admin-item",
            h3 { "{item.name}" }
            div { class: "admin-controls",
                div { class: "control-group",
                    label { "Ціна (грн/{unit}):" }
                    input {
                        class: "price-input",
                        value: "{price_input}",
                        oninput: move |e| price_input.set(e.value())
                    }
                    button { onclick: handle_save_price, "Зберегти" }
                }

                div { class: "status-group",
                    p { "Залишок: {item.stored} / {item.capacity} {unit}" }

                    if !is_electric {
                        div { class: "refill-control",
                            label { "Поповнити: {refill_amount} {unit}" }
                            input {
                                type: "range",
                                min: "10",
                                max: "1000",
                                step: "10",
                                value: "{refill_amount}",
                                oninput: move |e| refill_amount.set(e.value().parse().unwrap_or(10))
                            }
                            p { class: "cost-preview", "Вартість: {fmt_money(refill_cost)} грн" }
                            button { class: "refill-btn", onclick: handle_refill, "Поповнити" }
                        }
                    }
                }

                if !msg().is_empty() {
                    div { class: "status-msg", "{msg}" }
                }
            }
        }
    }
}
