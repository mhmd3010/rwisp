use sqlx::mysql::MySqlRow;
use sqlx::{FromRow, MySqlPool, Row};

use crate::packages::error::PackageError;
use crate::packages::model::{Package, PackageInput};

macro_rules! select_package_sql {
    ($suffix:literal) => {
        concat!(
            "SELECT id, name, description, price_cents, enabled, validity_secs, usage_limit_secs, ",
            "data_limit_bytes, upload_rate_bps, download_rate_bps, device_limit, ",
            "CAST(created_at AS CHAR) AS created_at, CAST(updated_at AS CHAR) AS updated_at ",
            "FROM tbl_packages ",
            $suffix
        )
    };
}

const SELECT_ROUTER_PACKAGES: &str = "SELECT p.id, p.name, p.description, p.price_cents, p.enabled, \
    p.validity_secs, p.usage_limit_secs, p.data_limit_bytes, p.upload_rate_bps, \
    p.download_rate_bps, p.device_limit, CAST(p.created_at AS CHAR) AS created_at, \
    CAST(p.updated_at AS CHAR) AS updated_at FROM tbl_packages p \
    JOIN tbl_router_packages rp ON rp.package_id = p.id WHERE rp.router_id = ? ORDER BY p.name";

const SELECT_ROUTER_PACKAGES_ENABLED: &str = "SELECT p.id, p.name, p.description, p.price_cents, \
    p.enabled, p.validity_secs, p.usage_limit_secs, p.data_limit_bytes, p.upload_rate_bps, \
    p.download_rate_bps, p.device_limit, CAST(p.created_at AS CHAR) AS created_at, \
    CAST(p.updated_at AS CHAR) AS updated_at FROM tbl_packages p \
    JOIN tbl_router_packages rp ON rp.package_id = p.id \
    WHERE rp.router_id = ? AND p.enabled = 1 ORDER BY p.name";

impl FromRow<'_, MySqlRow> for Package {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        let enabled_raw: i8 = row.try_get("enabled")?;
        let validity_secs: Option<u32> = row.try_get("validity_secs")?;
        let usage_limit_secs: Option<u32> = row.try_get("usage_limit_secs")?;

        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            price_cents: row.try_get("price_cents")?,
            enabled: enabled_raw != 0,
            validity_secs: validity_secs.map(u64::from),
            usage_limit_secs: usage_limit_secs.map(u64::from),
            data_limit_bytes: row.try_get("data_limit_bytes")?,
            upload_rate_bps: row.try_get("upload_rate_bps")?,
            download_rate_bps: row.try_get("download_rate_bps")?,
            device_limit: row.try_get("device_limit")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

pub async fn create_package(pool: &MySqlPool, input: &PackageInput) -> Result<u64, PackageError> {
    let result = sqlx::query(
        "INSERT INTO tbl_packages \
         (name, description, price_cents, enabled, validity_secs, usage_limit_secs, \
          data_limit_bytes, upload_rate_bps, download_rate_bps, device_limit) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&input.name)
    .bind(&input.description)
    .bind(input.price_cents)
    .bind(i8::from(input.enabled))
    .bind(input.validity_secs)
    .bind(input.usage_limit_secs)
    .bind(input.data_limit_bytes)
    .bind(input.upload_rate_bps)
    .bind(input.download_rate_bps)
    .bind(input.device_limit)
    .execute(pool)
    .await
    .map_err(map_write_error)?;

    Ok(result.last_insert_id())
}

pub async fn get_package(pool: &MySqlPool, id: u64) -> Result<Option<Package>, PackageError> {
    let record = sqlx::query_as::<_, Package>(select_package_sql!("WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(record)
}

pub async fn list_packages(
    pool: &MySqlPool,
    include_disabled: bool,
) -> Result<Vec<Package>, PackageError> {
    let query = if include_disabled {
        select_package_sql!("ORDER BY name")
    } else {
        select_package_sql!("WHERE enabled = 1 ORDER BY name")
    };

    let records = sqlx::query_as::<_, Package>(query).fetch_all(pool).await?;
    Ok(records)
}

pub async fn update_package(
    pool: &MySqlPool,
    id: u64,
    input: &PackageInput,
) -> Result<bool, PackageError> {
    let result = sqlx::query(
        "UPDATE tbl_packages SET name = ?, description = ?, price_cents = ?, enabled = ?, \
         validity_secs = ?, usage_limit_secs = ?, data_limit_bytes = ?, upload_rate_bps = ?, \
         download_rate_bps = ?, device_limit = ? WHERE id = ?",
    )
    .bind(&input.name)
    .bind(&input.description)
    .bind(input.price_cents)
    .bind(i8::from(input.enabled))
    .bind(input.validity_secs)
    .bind(input.usage_limit_secs)
    .bind(input.data_limit_bytes)
    .bind(input.upload_rate_bps)
    .bind(input.download_rate_bps)
    .bind(input.device_limit)
    .bind(id)
    .execute(pool)
    .await
    .map_err(map_write_error)?;

    Ok(result.rows_affected() > 0)
}

pub async fn delete_package(pool: &MySqlPool, id: u64) -> Result<bool, PackageError> {
    let result = sqlx::query("DELETE FROM tbl_packages WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn assign_package(
    pool: &MySqlPool,
    router_id: u64,
    package_id: u64,
) -> Result<(), PackageError> {
    sqlx::query(
        "INSERT INTO tbl_router_packages (router_id, package_id) VALUES (?, ?) \
         ON DUPLICATE KEY UPDATE package_id = package_id",
    )
    .bind(router_id)
    .bind(package_id)
    .execute(pool)
    .await
    .map_err(map_write_error)?;

    Ok(())
}

pub async fn unassign_package(
    pool: &MySqlPool,
    router_id: u64,
    package_id: u64,
) -> Result<bool, PackageError> {
    let result =
        sqlx::query("DELETE FROM tbl_router_packages WHERE router_id = ? AND package_id = ?")
            .bind(router_id)
            .bind(package_id)
            .execute(pool)
            .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn list_router_package_ids(
    pool: &MySqlPool,
    router_id: u64,
) -> Result<Vec<u64>, PackageError> {
    let ids = sqlx::query_scalar::<_, u64>(
        "SELECT package_id FROM tbl_router_packages WHERE router_id = ? ORDER BY package_id",
    )
    .bind(router_id)
    .fetch_all(pool)
    .await?;

    Ok(ids)
}

pub async fn list_packages_for_router(
    pool: &MySqlPool,
    router_id: u64,
    include_disabled: bool,
) -> Result<Vec<Package>, PackageError> {
    let query = if include_disabled {
        SELECT_ROUTER_PACKAGES
    } else {
        SELECT_ROUTER_PACKAGES_ENABLED
    };

    let records = sqlx::query_as::<_, Package>(query)
        .bind(router_id)
        .fetch_all(pool)
        .await?;

    Ok(records)
}

pub async fn package_is_global(pool: &MySqlPool, package_id: u64) -> Result<bool, PackageError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tbl_router_packages WHERE package_id = ?")
            .bind(package_id)
            .fetch_one(pool)
            .await?;

    Ok(count == 0)
}

fn map_write_error(error: sqlx::Error) -> PackageError {
    if let sqlx::Error::Database(ref database_error) = error {
        if database_error.is_unique_violation() {
            return PackageError::Conflict {
                message: "a package with this name already exists".to_string(),
            };
        }
        if database_error.is_foreign_key_violation() {
            return PackageError::Validation {
                field: "assignment",
                message: "router or package does not exist".to_string(),
            };
        }
    }
    PackageError::Database(error)
}
