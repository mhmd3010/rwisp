use sqlx::{MySqlPool, Row};

use crate::accounts;
use crate::radius::accounting;
use crate::radius::coa::DisconnectTarget;
use crate::radius::error::RadiusError;
use crate::vouchers;

#[derive(Debug, Default)]
pub struct MaintenanceReport {
    pub subscriptions_expired: u64,
    pub vouchers_expired: u64,
    pub renewals_activated: u64,
    pub disconnects: Vec<DisconnectTarget>,
}

/// Periodic lifecycle processing: expire overdue subscriptions/vouchers,
/// promote due scheduled renewals, and list sessions that must be disconnected.
pub async fn run_maintenance(pool: &MySqlPool) -> Result<MaintenanceReport, RadiusError> {
    let subscriptions_expired =
        accounts::expire_overdue_subscriptions(pool)
            .await
            .map_err(|error| RadiusError::Internal {
                message: error.to_string(),
            })?;
    let vouchers_expired =
        vouchers::expire_overdue(pool)
            .await
            .map_err(|error| RadiusError::Internal {
                message: error.to_string(),
            })?;
    let renewals_activated = activate_due_scheduled_renewals(pool).await?;
    let disconnects = accounting::open_disconnect_candidates(pool).await?;

    Ok(MaintenanceReport {
        subscriptions_expired,
        vouchers_expired,
        renewals_activated,
        disconnects,
    })
}

/// Promote scheduled renewals whose start time has arrived.
///
/// The account row is locked per activation so this is safe to run alongside
/// live authentication, and suspended accounts stay suspended.
pub async fn activate_due_scheduled_renewals(pool: &MySqlPool) -> Result<u64, RadiusError> {
    let due = sqlx::query(
        "SELECT ap.id AS assignment_id, ap.account_id \
         FROM tbl_account_packages ap \
         JOIN tbl_accounts a ON a.id = ap.account_id \
         WHERE ap.status = 'scheduled' AND ap.starts_at IS NOT NULL AND ap.starts_at <= NOW() \
           AND a.status IN ('active', 'suspended') \
         ORDER BY ap.starts_at ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut activated = 0u64;
    for row in due {
        let assignment_id: u64 = row.try_get("assignment_id")?;
        let account_id: u64 = row.try_get("account_id")?;

        let mut tx = pool.begin().await?;
        let status: String =
            sqlx::query_scalar("SELECT status FROM tbl_accounts WHERE id = ? FOR UPDATE")
                .bind(account_id)
                .fetch_one(&mut *tx)
                .await?;

        let updated = sqlx::query(
            "UPDATE tbl_account_packages ap JOIN tbl_packages p ON p.id = ap.package_id \
             SET ap.status = 'active', ap.activated_at = NOW(), \
                 ap.expires_at = CASE WHEN p.validity_secs IS NULL THEN NULL \
                                      ELSE DATE_ADD(ap.starts_at, INTERVAL p.validity_secs SECOND) END \
             WHERE ap.id = ? AND ap.status = 'scheduled'",
        )
        .bind(assignment_id)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            tx.rollback().await?;
            continue;
        }

        sqlx::query(
            "UPDATE tbl_account_packages SET status = 'completed' \
             WHERE account_id = ? AND status IN ('active', 'expired') AND id <> ?",
        )
        .bind(account_id)
        .bind(assignment_id)
        .execute(&mut *tx)
        .await?;

        let new_status = if status == "suspended" {
            "suspended"
        } else {
            "active"
        };
        sqlx::query(
            "UPDATE tbl_accounts a JOIN tbl_account_packages ap ON ap.id = ? \
             SET a.package_id = ap.package_id, a.expires_at = ap.expires_at, a.status = ?, \
                 a.activated_at = COALESCE(a.activated_at, NOW()) \
             WHERE a.id = ?",
        )
        .bind(assignment_id)
        .bind(new_status)
        .bind(account_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        activated += 1;
    }

    Ok(activated)
}
