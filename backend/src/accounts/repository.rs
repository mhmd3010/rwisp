use sqlx::mysql::MySqlRow;
use sqlx::{FromRow, MySqlPool, QueryBuilder, Row};

use crate::accounts::error::AccountError;
use crate::accounts::model::{
    Account, AccountFilter, AccountPackage, AccountPackageStatus, AccountStatus, AccountUpdate,
    ActivationOutcome, NewAccount, PackageActivation, RenewalRequest, RenewalResult, StatusOutcome,
    SubscriptionStatus,
};
use crate::accounts::password::hash_password;

macro_rules! select_account_sql {
    ($suffix:literal) => {
        concat!(
            "SELECT id, username, full_name, email, phone, address, router_id, package_id, status, ",
            "CAST(activated_at AS CHAR) AS activated_at, CAST(expires_at AS CHAR) AS expires_at, ",
            "CAST(last_login_at AS CHAR) AS last_login_at, last_login_ip, usage_secs, bytes_in, ",
            "bytes_out, CAST(created_at AS CHAR) AS created_at, CAST(updated_at AS CHAR) AS updated_at, ",
            "COALESCE((SELECT CASE WHEN ap.status = 'active' AND ap.expires_at IS NOT NULL ",
            "AND ap.expires_at < NOW() THEN 'expired' ELSE ap.status END ",
            "FROM tbl_account_packages ap WHERE ap.account_id = tbl_accounts.id ",
            "AND ap.status IN ('active', 'scheduled') ",
            "ORDER BY (ap.status = 'active') DESC, ap.id DESC LIMIT 1), 'none') AS subscription_status ",
            "FROM tbl_accounts ",
            $suffix
        )
    };
}

const SELECT_ACCOUNTS: &str = concat!(
    "SELECT id, username, full_name, email, phone, address, router_id, package_id, status, ",
    "CAST(activated_at AS CHAR) AS activated_at, CAST(expires_at AS CHAR) AS expires_at, ",
    "CAST(last_login_at AS CHAR) AS last_login_at, last_login_ip, usage_secs, bytes_in, ",
    "bytes_out, CAST(created_at AS CHAR) AS created_at, CAST(updated_at AS CHAR) AS updated_at, ",
    "COALESCE((SELECT CASE WHEN ap.status = 'active' AND ap.expires_at IS NOT NULL ",
    "AND ap.expires_at < NOW() THEN 'expired' ELSE ap.status END ",
    "FROM tbl_account_packages ap WHERE ap.account_id = tbl_accounts.id ",
    "AND ap.status IN ('active', 'scheduled') ",
    "ORDER BY (ap.status = 'active') DESC, ap.id DESC LIMIT 1), 'none') AS subscription_status ",
    "FROM tbl_accounts WHERE 1 = 1"
);

macro_rules! select_account_package_sql {
    ($suffix:literal) => {
        concat!(
            "SELECT id, account_id, package_id, activation, status, ",
            "CAST(starts_at AS CHAR) AS starts_at, CAST(expires_at AS CHAR) AS expires_at, ",
            "CAST(activated_at AS CHAR) AS activated_at, CAST(created_at AS CHAR) AS created_at ",
            "FROM tbl_account_packages ",
            $suffix
        )
    };
}

impl FromRow<'_, MySqlRow> for Account {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        let status = decode_status(row)?;
        let subscription_raw: String = row.try_get("subscription_status")?;
        let subscription_status = SubscriptionStatus::parse(&subscription_raw)
            .ok_or_else(|| decode_error("subscription status", &subscription_raw))?;
        let usage_secs: u32 = row.try_get("usage_secs")?;

        Ok(Self {
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            full_name: row.try_get("full_name")?,
            email: row.try_get("email")?,
            phone: row.try_get("phone")?,
            address: row.try_get("address")?,
            router_id: row.try_get("router_id")?,
            package_id: row.try_get("package_id")?,
            status,
            subscription_status,
            activated_at: row.try_get("activated_at")?,
            expires_at: row.try_get("expires_at")?,
            last_login_at: row.try_get("last_login_at")?,
            last_login_ip: row.try_get("last_login_ip")?,
            usage_secs: u64::from(usage_secs),
            bytes_in: row.try_get("bytes_in")?,
            bytes_out: row.try_get("bytes_out")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl FromRow<'_, MySqlRow> for AccountPackage {
    fn from_row(row: &MySqlRow) -> Result<Self, sqlx::Error> {
        let activation_raw: String = row.try_get("activation")?;
        let activation = PackageActivation::parse(&activation_raw)
            .ok_or_else(|| decode_error("activation", &activation_raw))?;

        let status_raw: String = row.try_get("status")?;
        let status = AccountPackageStatus::parse(&status_raw)
            .ok_or_else(|| decode_error("package status", &status_raw))?;

        Ok(Self {
            id: row.try_get("id")?,
            account_id: row.try_get("account_id")?,
            package_id: row.try_get("package_id")?,
            activation,
            status,
            starts_at: row.try_get("starts_at")?,
            expires_at: row.try_get("expires_at")?,
            activated_at: row.try_get("activated_at")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

pub async fn create_account(pool: &MySqlPool, account: &NewAccount) -> Result<u64, AccountError> {
    let mut tx = pool.begin().await?;

    if let Some(router_id) = account.router_id
        && !row_exists(
            &mut tx,
            "SELECT COUNT(*) FROM tbl_routers WHERE id = ?",
            router_id,
        )
        .await?
    {
        return Err(AccountError::Validation {
            field: "router_id",
            message: "no such router".to_string(),
        });
    }
    if let Some(package_id) = account.package_id
        && !row_exists(
            &mut tx,
            "SELECT COUNT(*) FROM tbl_packages WHERE id = ?",
            package_id,
        )
        .await?
    {
        return Err(AccountError::Validation {
            field: "package_id",
            message: "no such package".to_string(),
        });
    }

    let password_hash = hash_password(&account.password)?;

    let result = sqlx::query(
        "INSERT INTO tbl_accounts \
         (username, password_hash, full_name, email, phone, address, router_id, package_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&account.username)
    .bind(&password_hash)
    .bind(&account.full_name)
    .bind(&account.email)
    .bind(&account.phone)
    .bind(&account.address)
    .bind(account.router_id)
    .bind(account.package_id)
    .execute(&mut *tx)
    .await
    .map_err(map_write_error)?;

    let id = result.last_insert_id();

    // Record the initial assignment as scheduled: validity only starts on first
    // successful login, but the history/audit trail exists immediately.
    if let Some(package_id) = account.package_id {
        sqlx::query(
            "INSERT INTO tbl_account_packages \
             (account_id, package_id, activation, status, starts_at, activated_at, expires_at) \
             VALUES (?, ?, 'immediate', 'scheduled', NULL, NULL, NULL)",
        )
        .bind(id)
        .bind(package_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(id)
}

pub async fn get_account(pool: &MySqlPool, id: u64) -> Result<Option<Account>, AccountError> {
    let account = sqlx::query_as::<_, Account>(select_account_sql!("WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(account)
}

pub async fn get_account_by_username(
    pool: &MySqlPool,
    username: &str,
) -> Result<Option<Account>, AccountError> {
    let account = sqlx::query_as::<_, Account>(select_account_sql!("WHERE username = ?"))
        .bind(username)
        .fetch_optional(pool)
        .await?;
    Ok(account)
}

pub async fn list_accounts(
    pool: &MySqlPool,
    filter: &AccountFilter,
) -> Result<Vec<Account>, AccountError> {
    let mut builder = QueryBuilder::new(SELECT_ACCOUNTS);

    if let Some(username) = &filter.username {
        builder
            .push(" AND username LIKE ")
            .push_bind(format!("%{username}%"));
    }
    if let Some(status) = filter.status {
        builder.push(" AND status = ").push_bind(status.as_str());
    }
    if let Some(package_id) = filter.package_id {
        builder.push(" AND package_id = ").push_bind(package_id);
    }
    if let Some(router_id) = filter.router_id {
        builder.push(" AND router_id = ").push_bind(router_id);
    }
    builder
        .push(" ORDER BY id DESC LIMIT ")
        .push_bind(i64::from(filter.limit))
        .push(" OFFSET ")
        .push_bind(i64::from(filter.offset));

    let accounts = builder.build_query_as::<Account>().fetch_all(pool).await?;
    Ok(accounts)
}

pub async fn update_account(
    pool: &MySqlPool,
    id: u64,
    update: &AccountUpdate,
) -> Result<bool, AccountError> {
    if let Some(router_id) = update.router_id
        && !row_exists_pool(
            pool,
            "SELECT COUNT(*) FROM tbl_routers WHERE id = ?",
            router_id,
        )
        .await?
    {
        return Err(AccountError::Validation {
            field: "router_id",
            message: "no such router".to_string(),
        });
    }

    let result = sqlx::query(
        "UPDATE tbl_accounts SET full_name = ?, email = ?, phone = ?, address = ?, router_id = ? \
         WHERE id = ?",
    )
    .bind(&update.full_name)
    .bind(&update.email)
    .bind(&update.phone)
    .bind(&update.address)
    .bind(update.router_id)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn reset_password(
    pool: &MySqlPool,
    id: u64,
    new_password: &str,
) -> Result<bool, AccountError> {
    let password_hash = hash_password(new_password)?;

    let result = sqlx::query("UPDATE tbl_accounts SET password_hash = ? WHERE id = ?")
        .bind(password_hash)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn verify_account_password(
    pool: &MySqlPool,
    username: &str,
    password: &str,
) -> Result<Option<Account>, AccountError> {
    let row = sqlx::query("SELECT password_hash FROM tbl_accounts WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    let password_hash: String = row.try_get("password_hash")?;

    if crate::accounts::password::verify_password(password, &password_hash)? {
        Ok(get_account_by_username(pool, username).await?)
    } else {
        Ok(None)
    }
}

pub async fn suspend_account(pool: &MySqlPool, id: u64) -> Result<StatusOutcome, AccountError> {
    let result = sqlx::query(
        "UPDATE tbl_accounts SET status = 'suspended' \
         WHERE id = ? AND status IN ('pending', 'active')",
    )
    .bind(id)
    .execute(pool)
    .await?;

    finish_status_outcome(pool, id, result.rows_affected() == 1).await
}

pub async fn reactivate_account(pool: &MySqlPool, id: u64) -> Result<StatusOutcome, AccountError> {
    let result = sqlx::query(
        "UPDATE tbl_accounts SET status = IF(activated_at IS NULL, 'pending', 'active') \
         WHERE id = ? AND status = 'suspended'",
    )
    .bind(id)
    .execute(pool)
    .await?;

    finish_status_outcome(pool, id, result.rows_affected() == 1).await
}

pub async fn delete_account(pool: &MySqlPool, id: u64) -> Result<bool, AccountError> {
    let result = sqlx::query("DELETE FROM tbl_accounts WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn activate_account(
    pool: &MySqlPool,
    id: u64,
) -> Result<ActivationOutcome, AccountError> {
    // Atomic first-login activation: the account row is locked for the whole
    // transition, so concurrent logins cannot both activate it, and the pending
    // assignment is promoted to active in the same transaction for consistent
    // history. Validity always starts now, never at creation.
    let mut tx = pool.begin().await?;

    let row = sqlx::query("SELECT status FROM tbl_accounts WHERE id = ? FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let Some(row) = row else {
        return Ok(ActivationOutcome::NotFound);
    };
    let status_raw: String = row.try_get("status")?;
    let status =
        AccountStatus::parse(&status_raw).ok_or_else(|| decode_error("status", &status_raw))?;

    if status != AccountStatus::Pending {
        tx.rollback().await?;
        return Ok(ActivationOutcome::NotPending { status });
    }

    let assignment_id: Option<u64> = sqlx::query_scalar(
        "SELECT id FROM tbl_account_packages WHERE account_id = ? AND status = 'scheduled' \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(assignment_id) = assignment_id {
        sqlx::query(
            "UPDATE tbl_account_packages ap JOIN tbl_packages p ON p.id = ap.package_id \
             SET ap.status = 'active', ap.starts_at = NOW(), ap.activated_at = NOW(), \
                 ap.expires_at = CASE WHEN p.validity_secs IS NULL THEN NULL \
                                      ELSE DATE_ADD(NOW(), INTERVAL p.validity_secs SECOND) END \
             WHERE ap.id = ?",
        )
        .bind(assignment_id)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "UPDATE tbl_accounts a LEFT JOIN tbl_packages p ON p.id = a.package_id \
         SET a.status = 'active', a.activated_at = NOW(), a.last_login_at = NOW(), \
             a.expires_at = CASE WHEN p.validity_secs IS NULL THEN NULL \
                                 ELSE DATE_ADD(NOW(), INTERVAL p.validity_secs SECOND) END \
         WHERE a.id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let account = get_account(pool, id)
        .await?
        .ok_or(AccountError::NotFound { id })?;
    Ok(ActivationOutcome::Activated(Box::new(account)))
}

pub async fn renew_account(
    pool: &MySqlPool,
    id: u64,
    request: &RenewalRequest,
) -> Result<RenewalResult, AccountError> {
    let mut tx = pool.begin().await?;

    let account_row = sqlx::query(
        "SELECT status, CAST(expires_at AS CHAR) AS expires_at FROM tbl_accounts WHERE id = ? FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(account_row) = account_row else {
        return Err(AccountError::NotFound { id });
    };
    let status_raw: String = account_row.try_get("status")?;
    let status = AccountStatus::parse(&status_raw)
        .ok_or_else(|| decode_error("account status", &status_raw))?;
    let current_expiry: Option<String> = account_row.try_get("expires_at")?;

    if status == AccountStatus::Disabled {
        return Err(AccountError::Conflict {
            message: "account is disabled and cannot be renewed".to_string(),
        });
    }

    let validity_raw: Option<Option<u32>> =
        sqlx::query_scalar::<_, Option<u32>>("SELECT validity_secs FROM tbl_packages WHERE id = ?")
            .bind(request.package_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(validity) = validity_raw else {
        return Err(AccountError::Validation {
            field: "package_id",
            message: "no such package".to_string(),
        });
    };

    let assignment_id = if status == AccountStatus::Pending {
        // Pending accounts never started validity: the newest package replaces
        // the previous pending assignment (recorded as cancelled) and validity
        // will still start at the first successful login.
        sqlx::query(
            "UPDATE tbl_account_packages SET status = 'cancelled' \
             WHERE account_id = ? AND status = 'scheduled'",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;

        let inserted = sqlx::query(
            "INSERT INTO tbl_account_packages \
             (account_id, package_id, activation, status, starts_at, activated_at, expires_at) \
             VALUES (?, ?, ?, 'scheduled', NULL, NULL, NULL)",
        )
        .bind(id)
        .bind(request.package_id)
        .bind(request.activation.as_str())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE tbl_accounts SET package_id = ?, expires_at = NULL, activated_at = NULL, \
             status = 'pending' WHERE id = ?",
        )
        .bind(request.package_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        inserted.last_insert_id()
    } else {
        match request.activation {
            PackageActivation::Immediate => {
                // Preserve unused validity: the new package extends from the
                // later of now and the current expiry, so remaining time is
                // never lost. An expired account simply restarts from now.
                let inserted = sqlx::query(
                    "INSERT INTO tbl_account_packages \
                     (account_id, package_id, activation, status, starts_at, activated_at, expires_at) \
                     VALUES (?, ?, 'immediate', 'active', NOW(), NOW(), \
                             CASE WHEN ? IS NULL THEN NULL \
                                  ELSE DATE_ADD(GREATEST(NOW(), COALESCE(?, NOW())), INTERVAL ? SECOND) END)",
                )
                .bind(id)
                .bind(request.package_id)
                .bind(validity)
                .bind(&current_expiry)
                .bind(validity)
                .execute(&mut *tx)
                .await?;
                let assignment_id = inserted.last_insert_id();

                sqlx::query(
                    "UPDATE tbl_account_packages SET status = 'completed' \
                     WHERE account_id = ? AND status = 'active' AND id <> ?",
                )
                .bind(id)
                .bind(assignment_id)
                .execute(&mut *tx)
                .await?;

                let new_status = if status == AccountStatus::Suspended {
                    "suspended"
                } else {
                    "active"
                };
                sqlx::query(
                    "UPDATE tbl_accounts SET package_id = ?, status = ?, \
                        activated_at = COALESCE(activated_at, NOW()), \
                        expires_at = CASE WHEN ? IS NULL THEN NULL \
                                          ELSE DATE_ADD(GREATEST(NOW(), COALESCE(?, NOW())), INTERVAL ? SECOND) END \
                     WHERE id = ?",
                )
                .bind(request.package_id)
                .bind(new_status)
                .bind(validity)
                .bind(&current_expiry)
                .bind(validity)
                .bind(id)
                .execute(&mut *tx)
                .await?;

                assignment_id
            }
            PackageActivation::NextExpiry => {
                // Scheduled to start when the current subscription ends; the
                // active subscription is left untouched.
                let inserted = sqlx::query(
                    "INSERT INTO tbl_account_packages \
                     (account_id, package_id, activation, status, starts_at, activated_at, expires_at) \
                     VALUES (?, ?, 'next_expiry', 'scheduled', COALESCE(?, NOW()), NULL, \
                             CASE WHEN ? IS NULL THEN NULL \
                                  ELSE DATE_ADD(COALESCE(?, NOW()), INTERVAL ? SECOND) END)",
                )
                .bind(id)
                .bind(request.package_id)
                .bind(&current_expiry)
                .bind(validity)
                .bind(&current_expiry)
                .bind(validity)
                .execute(&mut *tx)
                .await?;

                inserted.last_insert_id()
            }
        }
    };

    tx.commit().await?;

    let account = get_account(pool, id)
        .await?
        .ok_or(AccountError::NotFound { id })?;
    let assignment = get_account_package(pool, assignment_id)
        .await?
        .ok_or(AccountError::NotFound { id: assignment_id })?;

    Ok(RenewalResult {
        account,
        assignment,
    })
}

pub async fn list_account_packages(
    pool: &MySqlPool,
    account_id: u64,
) -> Result<Vec<AccountPackage>, AccountError> {
    let assignments = sqlx::query_as::<_, AccountPackage>(select_account_package_sql!(
        "WHERE account_id = ? ORDER BY id DESC"
    ))
    .bind(account_id)
    .fetch_all(pool)
    .await?;
    Ok(assignments)
}

pub async fn expire_overdue_subscriptions(pool: &MySqlPool) -> Result<u64, AccountError> {
    // Subscription expiry is tracked on assignments; the account lifecycle
    // status is left untouched (the API derives the expired view).
    let result = sqlx::query(
        "UPDATE tbl_account_packages SET status = 'expired' \
         WHERE status = 'active' AND expires_at IS NOT NULL AND expires_at < NOW()",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

async fn get_account_package(
    pool: &MySqlPool,
    id: u64,
) -> Result<Option<AccountPackage>, AccountError> {
    let assignment =
        sqlx::query_as::<_, AccountPackage>(select_account_package_sql!("WHERE id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(assignment)
}

async fn finish_status_outcome(
    pool: &MySqlPool,
    id: u64,
    updated: bool,
) -> Result<StatusOutcome, AccountError> {
    if updated {
        let account = get_account(pool, id)
            .await?
            .ok_or(AccountError::NotFound { id })?;
        return Ok(StatusOutcome::Updated(Box::new(account)));
    }

    match get_account(pool, id).await? {
        None => Ok(StatusOutcome::NotFound),
        Some(account) => Ok(StatusOutcome::InvalidState {
            status: account.status,
        }),
    }
}

async fn row_exists(
    tx: &mut sqlx::MySqlConnection,
    query: &'static str,
    id: u64,
) -> Result<bool, AccountError> {
    let count: i64 = sqlx::query_scalar(query).bind(id).fetch_one(tx).await?;
    Ok(count > 0)
}

async fn row_exists_pool(
    pool: &MySqlPool,
    query: &'static str,
    id: u64,
) -> Result<bool, AccountError> {
    let count: i64 = sqlx::query_scalar(query).bind(id).fetch_one(pool).await?;
    Ok(count > 0)
}

fn decode_status(row: &MySqlRow) -> Result<AccountStatus, sqlx::Error> {
    let raw: String = row.try_get("status")?;
    AccountStatus::parse(&raw).ok_or_else(|| decode_error("status", &raw))
}

fn decode_error(field: &str, value: &str) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("unknown {field} value: {value}"),
    )))
}

fn map_write_error(error: sqlx::Error) -> AccountError {
    if let sqlx::Error::Database(ref database_error) = error {
        if database_error.is_unique_violation() {
            return AccountError::Conflict {
                message: "an account with this username already exists".to_string(),
            };
        }
        if database_error.is_foreign_key_violation() {
            return AccountError::Validation {
                field: "reference",
                message: "referenced package or router does not exist".to_string(),
            };
        }
    }
    AccountError::Database(error)
}
