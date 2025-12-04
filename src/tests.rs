#[cfg(test)]
mod unit_tests {
    use crate::utils::*;

    #[test]
    fn test_price_calculation() {
        let price_per_liter = 5500;
        let liters = 10;
        // Test using real business logic from utils
        let total = calculate_fuel_cost(price_per_liter, liters);
        assert_eq!(total, 55000);
    }

    #[test]
    fn test_tank_percentage() {
        let stored = 500;
        let capacity = 1000;

        let percentage = calculate_tank_percentage(stored, capacity);
        assert_eq!(percentage, 50.0);
    }

    #[test]
    fn test_empty_tank() {
        let stored = 0;
        let capacity = 1000;
        let percentage = calculate_tank_percentage(stored, capacity);
        assert_eq!(percentage, 0.0);
    }

    #[test]
    fn test_full_tank() {
        let stored = 1000;
        let capacity = 1000;
        let percentage = calculate_tank_percentage(stored, capacity);
        assert_eq!(percentage, 100.0);
    }

    #[test]
    fn test_price_formatting() {
        let price = 5550;
        let formatted = format_money(price);
        assert_eq!(formatted, "55.50 грн");
    }

    #[test]
    fn test_refill_cost_calculation() {
        let price_per_liter = 10000;
        let liters = 100;
        let cost = calculate_refill_cost(price_per_liter, liters);
        assert_eq!(cost, 500000);
    }

    #[test]
    fn test_balance_check() {
        let balance1 = 100000;
        let balance2 = 40000;
        let cost = 50000;
        assert!(has_sufficient_balance(balance1, cost));
        assert!(!has_sufficient_balance(balance2, cost));
    }

    #[test]
    fn test_fuel_availability() {
        let tanks = vec![(500, 1000), (300, 1000), (200, 500)];
        let total = calculate_total_stored(&tanks);
        assert_eq!(total, 1000);
        assert!(has_sufficient_fuel(total, 500));
        assert!(!has_sufficient_fuel(total, 1500));
    }

    // Rate limiting tests
    #[test]
    fn test_rate_limit_allows_within_limit() {
        use crate::rate_limit::check_rate_limit;
        use std::time::Duration;

        let ip = "test_ip_1";
        assert!(check_rate_limit(ip, 5, Duration::from_secs(1)));
        assert!(check_rate_limit(ip, 5, Duration::from_secs(1)));
    }

    #[test]
    fn test_rate_limit_blocks_over_limit() {
        use crate::rate_limit::check_rate_limit;
        use std::time::Duration;

        let ip = "test_ip_2";
        for _ in 0..5 {
            assert!(check_rate_limit(ip, 5, Duration::from_secs(10)));
        }
        // 6-й запит має бути заблокований
        assert!(!check_rate_limit(ip, 5, Duration::from_secs(10)));
    }

    #[test]
    fn test_rate_limit_resets_after_window() {
        use crate::rate_limit::check_rate_limit;
        use std::thread;
        use std::time::Duration;

        let ip = "test_ip_3";
        for _ in 0..5 {
            check_rate_limit(ip, 5, Duration::from_millis(100));
        }

        // Чекаємо закінчення вікна
        thread::sleep(Duration::from_millis(150));

        // Запити дозволяються знову
        assert!(check_rate_limit(ip, 5, Duration::from_millis(100)));
    }
}

#[cfg(all(test, feature = "server"))]
mod integration_tests {
    use super::*;
    use crate::db;
    use crate::models::NewCustomer;
    use crate::schema::customer::dsl::*;
    use diesel::prelude::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_register_user_with_rollback() {
        dotenvy::dotenv().ok();
        let mut conn = db::connection();

        conn.test_transaction::<_, diesel::result::Error, _>(|conn| {
            let count_before = customer.count().get_result::<i64>(conn)?;
            println!("Користувачів до вставки: {}", count_before);

            let new_user = NewCustomer {
                login: "test_void",
                password: "hashed_password_123",
                salt: "bcrypt",
                balance: 1001,
            };

            diesel::insert_into(customer)
                .values(&new_user)
                .execute(conn)?;

            let count_during = customer.count().get_result::<i64>(conn)?;
            println!("Користувачів під час транзакції: {}", count_during);

            assert_eq!(
                count_during,
                count_before + 1,
                "Користувач має бути в базі!"
            );

            Ok(())
        });

        let count_after = customer
            .count()
            .get_result::<i64>(&mut conn)
            .expect("Error");
        println!("Користувачів після відкату: {}", count_after);

        let ghost_exists = customer
            .filter(login.eq("test_void"))
            .count()
            .get_result::<i64>(&mut conn)
            .unwrap();

        assert_eq!(ghost_exists, 0, "База даних має бути чистою після відкату!");
    }

    #[test]
    #[serial]
    fn test_sql_injection_in_login() {
        dotenvy::dotenv().ok();
        let mut conn = db::connection();

        // Спроба SQL injection через логін
        let malicious_login = "'admin' OR '1'='1' --";

        // Diesel автоматично екранує параметри, тому injection не спрацює
        let result = customer
            .filter(login.eq(malicious_login))
            .select(crate::models::Customer::as_select())
            .first(&mut conn)
            .optional();

        // Має повернути None або Ok(None), а не помилку
        assert!(
            result.is_ok(),
            "Diesel має безпечно обробити SQL injection спробу"
        );
        assert!(
            result.unwrap().is_none(),
            "Не має знайти користувача з malicious login"
        );
    }

    #[test]
    #[serial]
    fn test_sql_injection_in_insert() {
        dotenvy::dotenv().ok();
        let mut conn = db::connection();

        conn.test_transaction::<_, diesel::result::Error, _>(|conn| {
            // Спроба SQL injection через вставку
            let malicious_user = NewCustomer {
                login: "test'; DROP TABLE customer; --",
                password: "password",
                salt: "salt",
                balance: 1000,
            };

            // Diesel безпечно вставить це як звичайний текст
            let insert_result = diesel::insert_into(customer)
                .values(&malicious_user)
                .execute(conn);

            assert!(
                insert_result.is_ok(),
                "Diesel має безпечно обробити malicious input"
            );

            // Перевіримо що таблиця не видалена і запис створено
            let count = customer.count().get_result::<i64>(conn)?;
            assert!(count > 0, "Таблиця має існувати після спроби injection");

            Ok(())
        });

        // Перевіримо що таблиця все ще існує після транзакції
        let final_count = customer.count().get_result::<i64>(&mut conn);
        assert!(final_count.is_ok(), "Таблиця customer має існувати");
    }
}
