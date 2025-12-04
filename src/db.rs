#[cfg(feature = "server")]
pub mod server_db {
    use diesel::r2d2::{self, ConnectionManager};
    use diesel::RunQueryDsl;
    use diesel::SqliteConnection;
    use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
    use once_cell::sync::Lazy;

    pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

    pub type Pool = r2d2::Pool<ConnectionManager<SqliteConnection>>;

    pub static DB_POOL: Lazy<Pool> = Lazy::new(|| {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let manager = ConnectionManager::<SqliteConnection>::new(database_url);
        let pool = r2d2::Pool::builder()
            .build(manager)
            .expect("Failed to create pool.");

        // Налаштування SQLite для кращої продуктивності
        let mut conn = pool.get().expect("Failed to get connection");
        diesel::sql_query("PRAGMA journal_mode = WAL;")
            .execute(&mut conn)
            .ok();
        diesel::sql_query("PRAGMA synchronous = NORMAL;")
            .execute(&mut conn)
            .ok();
        diesel::sql_query("PRAGMA cache_size = 10000;")
            .execute(&mut conn)
            .ok();
        diesel::sql_query("PRAGMA temp_store = MEMORY;")
            .execute(&mut conn)
            .ok();

        pool
    });

    pub fn connection() -> diesel::r2d2::PooledConnection<ConnectionManager<SqliteConnection>> {
        DB_POOL.get().expect("Failed to get DB connection")
    }

    pub fn run_migrations() {
        let mut conn = connection();
        conn.run_pending_migrations(MIGRATIONS)
            .expect("Failed to run migrations");
        println!("LOG: Database migrations executed successfully.");
    }
}

#[cfg(feature = "server")]
pub use server_db::*;
