use std::time::Duration;

use sqlx::mysql::MySqlRow;
use sqlx::{FromRow, MySqlPool, Row};

use crate::crypto::EncryptionKey;
use crate::routers::error::RouterError;
use crate::routers::model::{NewRouter, RouterRecord, RouterUpdate};
use crate::rwispros::{RouterClient, RouterConfig, Transport};

const SELECT_ROUTER_WITH_CREDENTIALS: &str = "SELECT id, name, host, api_port, username, transport, \
    timeout_secs, radius_nas_ip, enabled, CAST(created_at AS CHAR) AS created_at, \
    CAST(updated_at AS CHAR) AS updated_at, password_encrypted FROM tbl_routers WHERE id = ?";

macro_rules! select_router_sql {
    ($suffix:literal) => {
        concat!(
            "SELECT id, name, host, api_port, username, transport, timeout_secs, radius_nas_ip, enabled, ",
            "CAST(created_at AS CHAR) AS created_at, CAST(updated_at AS CHAR) AS updated_at ",
            "FROM tbl_routers ",
            $suffix
        )
    };
}

impl FromRow<'_, MySqlRow> for RouterRecord {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        let transport_raw: String = row.try_get("transport")?;
        let transport = Transport::parse(&transport_raw).ok_or_else(|| {
            sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown router transport value: {transport_raw}"),
            )))
        })?;

        let timeout_secs: u32 = row.try_get("timeout_secs")?;
        let enabled_raw: i8 = row.try_get("enabled")?;

        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            host: row.try_get("host")?,
            api_port: row.try_get("api_port")?,
            username: row.try_get("username")?,
            transport,
            timeout: Duration::from_secs(u64::from(timeout_secs)),
            radius_nas_ip: row.try_get("radius_nas_ip")?,
            enabled: enabled_raw != 0,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

pub async fn create_router(
    pool: &MySqlPool,
    router: &NewRouter,
    key: &EncryptionKey,
) -> Result<u64, RouterError> {
    let encrypted = key.encrypt(router.password())?;

    let result = sqlx::query(
        "INSERT INTO tbl_routers \
         (name, host, api_port, username, password_encrypted, transport, timeout_secs, \
          radius_nas_ip, enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&router.name)
    .bind(&router.host)
    .bind(router.api_port)
    .bind(&router.username)
    .bind(encrypted)
    .bind(router.transport.as_str())
    .bind(router.timeout.as_secs() as u32)
    .bind(&router.radius_nas_ip)
    .bind(i8::from(router.enabled))
    .execute(pool)
    .await?;

    Ok(result.last_insert_id())
}

pub async fn get_router(pool: &MySqlPool, id: u64) -> Result<Option<RouterRecord>, RouterError> {
    let record = sqlx::query_as::<_, RouterRecord>(select_router_sql!("WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(record)
}

pub async fn list_routers(
    pool: &MySqlPool,
    include_disabled: bool,
) -> Result<Vec<RouterRecord>, RouterError> {
    let query = if include_disabled {
        select_router_sql!("ORDER BY name")
    } else {
        select_router_sql!("WHERE enabled = 1 ORDER BY name")
    };

    let records = sqlx::query_as::<_, RouterRecord>(query)
        .fetch_all(pool)
        .await?;
    Ok(records)
}

pub async fn update_router(
    pool: &MySqlPool,
    id: u64,
    update: &RouterUpdate,
    key: &EncryptionKey,
) -> Result<bool, RouterError> {
    let encrypted = match update.password() {
        Some(password) => Some(key.encrypt(password)?),
        None => None,
    };

    let result = sqlx::query(
        "UPDATE tbl_routers SET \
         name = ?, host = ?, api_port = ?, username = ?, \
         password_encrypted = COALESCE(?, password_encrypted), \
         transport = ?, timeout_secs = ?, radius_nas_ip = ?, enabled = ? \
         WHERE id = ?",
    )
    .bind(&update.name)
    .bind(&update.host)
    .bind(update.api_port)
    .bind(&update.username)
    .bind(encrypted)
    .bind(update.transport.as_str())
    .bind(update.timeout.as_secs() as u32)
    .bind(&update.radius_nas_ip)
    .bind(i8::from(update.enabled))
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn delete_router(pool: &MySqlPool, id: u64) -> Result<bool, RouterError> {
    let result = sqlx::query("DELETE FROM tbl_routers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn connect_router(
    pool: &MySqlPool,
    id: u64,
    key: &EncryptionKey,
) -> Result<RouterClient, RouterError> {
    let row = sqlx::query(SELECT_ROUTER_WITH_CREDENTIALS)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(RouterError::NotFound { id })?;

    let record = RouterRecord::from_row(&row)?;
    if !record.enabled {
        return Err(RouterError::Disabled { id });
    }

    let encrypted: Vec<u8> = row.try_get("password_encrypted")?;
    let password = key.decrypt(&encrypted)?;

    let config = RouterConfig::new(record.host, record.username, password)
        .with_port(record.api_port)
        .with_transport(record.transport)
        .with_timeout(record.timeout);

    let client = RouterClient::connect(config).await?;
    Ok(client)
}
