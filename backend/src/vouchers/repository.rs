use sqlx::mysql::MySqlRow;
use sqlx::{FromRow, MySqlPool, QueryBuilder, Row};

use crate::vouchers::code::generate_unique_codes;
use crate::vouchers::error::VoucherError;
use crate::vouchers::model::{
    ActivationOutcome, BatchRequest, GeneratedBatch, RevokeOutcome, Voucher, VoucherFilter,
    VoucherStatus,
};

const MAX_CODE_ATTEMPTS: u32 = 5;

macro_rules! select_voucher_sql {
    ($suffix:literal) => {
        concat!(
            "SELECT id, batch_id, code, package_id, router_id, status, ",
            "CAST(created_at AS CHAR) AS created_at, CAST(activated_at AS CHAR) AS activated_at, ",
            "CAST(expires_at AS CHAR) AS expires_at, CAST(last_used_at AS CHAR) AS last_used_at, ",
            "usage_secs, bytes_in, bytes_out, CAST(revoked_at AS CHAR) AS revoked_at ",
            "FROM tbl_vouchers ",
            $suffix
        )
    };
}

const SELECT_VOUCHERS: &str = concat!(
    "SELECT id, batch_id, code, package_id, router_id, status, ",
    "CAST(created_at AS CHAR) AS created_at, CAST(activated_at AS CHAR) AS activated_at, ",
    "CAST(expires_at AS CHAR) AS expires_at, CAST(last_used_at AS CHAR) AS last_used_at, ",
    "usage_secs, bytes_in, bytes_out, CAST(revoked_at AS CHAR) AS revoked_at ",
    "FROM tbl_vouchers WHERE 1 = 1"
);

impl FromRow<'_, MySqlRow> for Voucher {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        let status_raw: String = row.try_get("status")?;
        let status = VoucherStatus::parse(&status_raw).ok_or_else(|| {
            sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown voucher status: {status_raw}"),
            )))
        })?;

        let usage_secs: u32 = row.try_get("usage_secs")?;

        Ok(Self {
            id: row.try_get("id")?,
            batch_id: row.try_get("batch_id")?,
            code: row.try_get("code")?,
            package_id: row.try_get("package_id")?,
            router_id: row.try_get("router_id")?,
            status,
            created_at: row.try_get("created_at")?,
            activated_at: row.try_get("activated_at")?,
            expires_at: row.try_get("expires_at")?,
            last_used_at: row.try_get("last_used_at")?,
            usage_secs: u64::from(usage_secs),
            bytes_in: row.try_get("bytes_in")?,
            bytes_out: row.try_get("bytes_out")?,
            revoked_at: row.try_get("revoked_at")?,
        })
    }
}

pub async fn create_batch(
    pool: &MySqlPool,
    request: &BatchRequest,
) -> Result<GeneratedBatch, VoucherError> {
    let mut tx = pool.begin().await?;

    if !row_exists(
        &mut tx,
        "SELECT COUNT(*) FROM tbl_packages WHERE id = ?",
        request.package_id,
    )
    .await?
    {
        return Err(VoucherError::Validation {
            field: "package_id",
            message: "no such package".to_string(),
        });
    }
    if let Some(router_id) = request.router_id
        && !row_exists(
            &mut tx,
            "SELECT COUNT(*) FROM tbl_routers WHERE id = ?",
            router_id,
        )
        .await?
    {
        return Err(VoucherError::Validation {
            field: "router_id",
            message: "no such router".to_string(),
        });
    }

    let batch = sqlx::query(
        "INSERT INTO tbl_voucher_batches (package_id, router_id, quantity, note) VALUES (?, ?, ?, ?)",
    )
    .bind(request.package_id)
    .bind(request.router_id)
    .bind(request.quantity)
    .bind(&request.note)
    .execute(&mut *tx)
    .await?;
    let batch_id = batch.last_insert_id();

    let mut codes = generate_unique_codes(request.quantity as usize);
    let mut attempt = 0;
    loop {
        attempt += 1;

        let mut builder =
            QueryBuilder::new("INSERT INTO tbl_vouchers (batch_id, code, package_id, router_id) ");
        builder.push_values(codes.iter(), |mut row, code| {
            row.push_bind(batch_id)
                .push_bind(code.as_str())
                .push_bind(request.package_id)
                .push_bind(request.router_id);
        });

        match builder.build().execute(&mut *tx).await {
            Ok(_) => break,
            Err(error) if is_unique_violation(&error) && attempt < MAX_CODE_ATTEMPTS => {
                codes = generate_unique_codes(request.quantity as usize);
            }
            Err(error) => return Err(map_write_error(error)),
        }
    }

    tx.commit().await?;

    Ok(GeneratedBatch {
        batch_id,
        quantity: request.quantity,
        codes,
    })
}

pub async fn get_voucher(pool: &MySqlPool, id: u64) -> Result<Option<Voucher>, VoucherError> {
    let voucher = sqlx::query_as::<_, Voucher>(select_voucher_sql!("WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(voucher)
}

pub async fn get_voucher_by_code(
    pool: &MySqlPool,
    code: &str,
) -> Result<Option<Voucher>, VoucherError> {
    let voucher = sqlx::query_as::<_, Voucher>(select_voucher_sql!("WHERE code = ?"))
        .bind(code)
        .fetch_optional(pool)
        .await?;
    Ok(voucher)
}

pub async fn list_vouchers(
    pool: &MySqlPool,
    filter: &VoucherFilter,
) -> Result<Vec<Voucher>, VoucherError> {
    let mut builder = QueryBuilder::new(SELECT_VOUCHERS);

    if let Some(package_id) = filter.package_id {
        builder.push(" AND package_id = ").push_bind(package_id);
    }
    if let Some(router_id) = filter.router_id {
        builder.push(" AND router_id = ").push_bind(router_id);
    }
    if let Some(batch_id) = filter.batch_id {
        builder.push(" AND batch_id = ").push_bind(batch_id);
    }
    if let Some(status) = filter.status {
        builder.push(" AND status = ").push_bind(status.as_str());
    }
    builder
        .push(" ORDER BY id DESC LIMIT ")
        .push_bind(i64::from(filter.limit))
        .push(" OFFSET ")
        .push_bind(i64::from(filter.offset));

    let vouchers = builder.build_query_as::<Voucher>().fetch_all(pool).await?;
    Ok(vouchers)
}

pub async fn activate_voucher(
    pool: &MySqlPool,
    code: &str,
) -> Result<ActivationOutcome, VoucherError> {
    // A single conditional UPDATE is atomic: exactly one concurrent caller can
    // move a voucher from `unused` to `active`, so validity starts at the first
    // successful login and double activation is impossible.
    let result = sqlx::query(
        "UPDATE tbl_vouchers v JOIN tbl_packages p ON p.id = v.package_id \
         SET v.status = 'active', \
             v.activated_at = NOW(), \
             v.last_used_at = NOW(), \
             v.expires_at = IF(p.validity_secs IS NULL, NULL, \
                               DATE_ADD(NOW(), INTERVAL p.validity_secs SECOND)) \
         WHERE v.code = ? AND v.status = 'unused'",
    )
    .bind(code)
    .execute(pool)
    .await?;

    if result.rows_affected() == 1 {
        let voucher = get_voucher_by_code(pool, code)
            .await?
            .ok_or(VoucherError::NotFound { id: 0 })?;
        return Ok(ActivationOutcome::Activated(Box::new(voucher)));
    }

    match get_voucher_by_code(pool, code).await? {
        None => Ok(ActivationOutcome::NotFound),
        Some(voucher) => Ok(ActivationOutcome::NotUnused {
            status: voucher.status,
        }),
    }
}

pub async fn revoke_voucher(pool: &MySqlPool, id: u64) -> Result<RevokeOutcome, VoucherError> {
    let result = sqlx::query(
        "UPDATE tbl_vouchers SET status = 'revoked', revoked_at = NOW() \
         WHERE id = ? AND status IN ('unused', 'active')",
    )
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 1 {
        let voucher = get_voucher(pool, id)
            .await?
            .ok_or(VoucherError::NotFound { id })?;
        return Ok(RevokeOutcome::Revoked(Box::new(voucher)));
    }

    match get_voucher(pool, id).await? {
        None => Ok(RevokeOutcome::NotFound),
        Some(voucher) => Ok(RevokeOutcome::NotRevocable {
            status: voucher.status,
        }),
    }
}

pub async fn expire_overdue(pool: &MySqlPool) -> Result<u64, VoucherError> {
    let result = sqlx::query(
        "UPDATE tbl_vouchers SET status = 'expired' \
         WHERE status = 'active' AND expires_at IS NOT NULL AND expires_at < NOW()",
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn list_batch_codes(
    pool: &MySqlPool,
    batch_id: u64,
) -> Result<Vec<String>, VoucherError> {
    let codes = sqlx::query_scalar::<_, String>(
        "SELECT code FROM tbl_vouchers WHERE batch_id = ? ORDER BY id",
    )
    .bind(batch_id)
    .fetch_all(pool)
    .await?;

    Ok(codes)
}

pub async fn batch_exists(pool: &MySqlPool, batch_id: u64) -> Result<bool, VoucherError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_voucher_batches WHERE id = ?")
        .bind(batch_id)
        .fetch_one(pool)
        .await?;
    Ok(count > 0)
}

async fn row_exists(
    tx: &mut sqlx::MySqlConnection,
    query: &'static str,
    id: u64,
) -> Result<bool, VoucherError> {
    let count: i64 = sqlx::query_scalar(query).bind(id).fetch_one(tx).await?;
    Ok(count > 0)
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_unique_violation())
}

fn map_write_error(error: sqlx::Error) -> VoucherError {
    if let sqlx::Error::Database(ref database_error) = error {
        if database_error.is_unique_violation() {
            return VoucherError::Conflict {
                message: "a voucher with this code already exists".to_string(),
            };
        }
        if database_error.is_foreign_key_violation() {
            return VoucherError::Validation {
                field: "reference",
                message: "referenced package or router does not exist".to_string(),
            };
        }
    }
    VoucherError::Database(error)
}
