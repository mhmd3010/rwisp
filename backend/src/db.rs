use sqlx::MySqlPool;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use std::error::Error;
use std::time::Duration;

pub async fn init_pool() -> Result<MySqlPool, Box<dyn Error>> {
    let host =
        std::env::var("DATABASE_HOST").map_err(|_| "DATABASE_HOST is not set".to_string())?;
    let user =
        std::env::var("DATABASE_USER").map_err(|_| "DATABASE_USER is not set".to_string())?;
    let password = std::env::var("DATABASE_PASSWORD")
        .map_err(|_| "DATABASE_PASSWORD is not set".to_string())?;
    let database =
        std::env::var("DATABASE_NAME").map_err(|_| "DATABASE_NAME is not set".to_string())?;
    let port: u16 = std::env::var("DATABASE_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3306);

    let options = MySqlConnectOptions::new()
        .host(&host)
        .port(port)
        .username(&user)
        .password(&password)
        .database(&database)
        .charset("utf8mb4");

    let pool = MySqlPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;

    Ok(pool)
}
