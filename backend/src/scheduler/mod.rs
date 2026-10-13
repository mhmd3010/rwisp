use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::{MySqlPool, Row};
use tokio::sync::watch;

use crate::disconnects::{self, DisconnectMethod, NewDisconnect, SenderConfig};
use crate::radius;
use crate::{accounts, crypto::EncryptionKey, vouchers};

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub owner: String,
    pub tick_secs: u64,
    pub lease_secs: u64,
    pub disconnect_batch: u32,
    pub router_status_retention_days: u32,
    pub encryption_key: Option<Arc<EncryptionKey>>,
    pub sender: SenderConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    ExpireSubscriptions,
    ExpireVouchers,
    ActivateRenewals,
    EnqueueDisconnects,
    ProcessDisconnects,
    DetectStaleSessions,
    RouterHealth,
    NasSync,
    Cleanup,
}

impl Job {
    fn name(&self) -> &'static str {
        match self {
            Self::ExpireSubscriptions => "expire_subscriptions",
            Self::ExpireVouchers => "expire_vouchers",
            Self::ActivateRenewals => "activate_renewals",
            Self::EnqueueDisconnects => "enqueue_disconnects",
            Self::ProcessDisconnects => "process_disconnects",
            Self::DetectStaleSessions => "detect_stale_sessions",
            Self::RouterHealth => "router_health",
            Self::NasSync => "nas_sync",
            Self::Cleanup => "cleanup",
        }
    }

    fn interval_secs(&self) -> u64 {
        match self {
            Self::ExpireSubscriptions => 60,
            Self::ExpireVouchers => 60,
            Self::ActivateRenewals => 60,
            Self::EnqueueDisconnects => 15,
            Self::ProcessDisconnects => 10,
            Self::DetectStaleSessions => 300,
            Self::RouterHealth => 120,
            Self::NasSync => 300,
            Self::Cleanup => 3600,
        }
    }

    fn all() -> [Job; 9] {
        [
            Job::ExpireSubscriptions,
            Job::ExpireVouchers,
            Job::ActivateRenewals,
            Job::EnqueueDisconnects,
            Job::ProcessDisconnects,
            Job::DetectStaleSessions,
            Job::RouterHealth,
            Job::NasSync,
            Job::Cleanup,
        ]
    }
}

/// Run the scheduler until `shutdown` flips to true. Each job takes a database
/// lease so multiple instances never run the same job concurrently.
pub async fn run(pool: MySqlPool, config: SchedulerConfig, mut shutdown: watch::Receiver<bool>) {
    println!("RWISP scheduler started (owner={})", config.owner);
    let mut last_run: std::collections::HashMap<&'static str, Instant> = Default::default();
    let tick = Duration::from_secs(config.tick_secs.max(1));

    loop {
        if *shutdown.borrow() {
            println!("RWISP scheduler stopping");
            return;
        }

        for job in Job::all() {
            let due = last_run
                .get(job.name())
                .map(|last| last.elapsed().as_secs() >= job.interval_secs())
                .unwrap_or(true);
            if !due {
                continue;
            }
            last_run.insert(job.name(), Instant::now());

            match try_acquire(&pool, job.name(), &config.owner, config.lease_secs).await {
                Ok(true) => {
                    let detail = run_job(&pool, job, &config).await;
                    let (status, detail_text, error) = match detail {
                        Ok(text) => ("ok", text, None),
                        Err(error) => ("error", String::new(), Some(error)),
                    };
                    if let Err(error) = release(
                        &pool,
                        job.name(),
                        &config.owner,
                        status,
                        error.as_deref(),
                        &detail_text,
                    )
                    .await
                    {
                        eprintln!("scheduler: failed to record {}: {error}", job.name());
                    }
                }
                Ok(false) => {}
                Err(error) => eprintln!("scheduler: lease error for {}: {error}", job.name()),
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(tick) => {}
            _ = shutdown.changed() => {}
        }
    }
}

async fn take_lease(
    pool: &MySqlPool,
    job: &str,
    owner: &str,
    lease_secs: u64,
) -> Result<bool, sqlx::Error> {
    sqlx::query("INSERT IGNORE INTO tbl_scheduler_state (job_name) VALUES (?)")
        .bind(job)
        .execute(pool)
        .await?;
    let result = sqlx::query(
        "UPDATE tbl_scheduler_state SET lease_owner = ?, \
            lease_until = DATE_ADD(NOW(), INTERVAL ? SECOND), \
            last_started_at = NOW(), runs = runs + 1 \
         WHERE job_name = ? AND (lease_until IS NULL OR lease_until < NOW())",
    )
    .bind(owner)
    .bind(lease_secs)
    .bind(job)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

async fn try_acquire(
    pool: &MySqlPool,
    job: &str,
    owner: &str,
    lease_secs: u64,
) -> Result<bool, sqlx::Error> {
    take_lease(pool, job, owner, lease_secs).await
}

async fn release(
    pool: &MySqlPool,
    job: &str,
    owner: &str,
    status: &str,
    error: Option<&str>,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE tbl_scheduler_state SET lease_owner = NULL, lease_until = NULL, \
            last_finished_at = NOW(), last_status = ?, last_error = ?, last_detail = ? \
         WHERE job_name = ? AND lease_owner = ?",
    )
    .bind(status)
    .bind(error)
    .bind(detail)
    .bind(job)
    .bind(owner)
    .execute(pool)
    .await?;
    Ok(())
}

async fn run_job(pool: &MySqlPool, job: Job, config: &SchedulerConfig) -> Result<String, String> {
    match job {
        Job::ExpireSubscriptions => {
            let count = accounts::expire_overdue_subscriptions(pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("expired={count}"))
        }
        Job::ExpireVouchers => {
            let count = vouchers::expire_overdue(pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("expired={count}"))
        }
        Job::ActivateRenewals => {
            let count = radius::activate_due_scheduled_renewals(pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("activated={count}"))
        }
        Job::EnqueueDisconnects => {
            let count = enqueue_disconnect_candidates(pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("enqueued={count}"))
        }
        Job::ProcessDisconnects => {
            let outcome = process_disconnects(pool, config)
                .await
                .map_err(|e| e.to_string())?;
            Ok(outcome)
        }
        Job::DetectStaleSessions => {
            let stale = crate::sessions::stale_sessions(pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("stale={}", stale.len()))
        }
        Job::RouterHealth => {
            let checked = check_routers(pool, config)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("checked={checked}"))
        }
        Job::NasSync => {
            let synced = sync_nas(pool, config).await.map_err(|e| e.to_string())?;
            Ok(format!("synced={synced}"))
        }
        Job::Cleanup => {
            let pruned = cleanup(pool, config.router_status_retention_days)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("router_status_pruned={pruned}"))
        }
    }
}

async fn enqueue_disconnect_candidates(pool: &MySqlPool) -> Result<u64, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT s.id, s.username, s.account_id, s.voucher_id, s.nas_ip, s.acct_session_id \
         FROM tbl_radius_sessions s \
         LEFT JOIN tbl_accounts a ON a.id = s.account_id \
         LEFT JOIN tbl_vouchers v ON v.id = s.voucher_id \
         LEFT JOIN tbl_packages ap ON ap.id = a.package_id \
         LEFT JOIN tbl_packages vp ON vp.id = v.package_id \
         WHERE s.status <> 'stop' AND ( \
            a.status IN ('suspended', 'disabled') OR v.status IN ('revoked', 'expired') \
            OR (a.id IS NOT NULL AND a.expires_at IS NOT NULL AND a.expires_at < NOW()) \
            OR (v.id IS NOT NULL AND v.expires_at IS NOT NULL AND v.expires_at < NOW()) \
            OR (a.id IS NOT NULL AND ap.data_limit_bytes IS NOT NULL AND \
                (SELECT COALESCE(SUM(ss.input_octets + ss.output_octets), 0) FROM tbl_radius_sessions ss \
                 WHERE ss.account_id = a.id) >= ap.data_limit_bytes) \
            OR (a.id IS NOT NULL AND ap.usage_limit_secs IS NOT NULL AND \
                (SELECT COALESCE(SUM(ss.session_time), 0) FROM tbl_radius_sessions ss \
                 WHERE ss.account_id = a.id) >= ap.usage_limit_secs) \
            OR (v.id IS NOT NULL AND vp.data_limit_bytes IS NOT NULL AND \
                (SELECT COALESCE(SUM(ss.input_octets + ss.output_octets), 0) FROM tbl_radius_sessions ss \
                 WHERE ss.voucher_id = v.id) >= vp.data_limit_bytes) \
            OR (v.id IS NOT NULL AND vp.usage_limit_secs IS NOT NULL AND \
                (SELECT COALESCE(SUM(ss.session_time), 0) FROM tbl_radius_sessions ss \
                 WHERE ss.voucher_id = v.id) >= vp.usage_limit_secs) \
         ) \
         LIMIT 500",
    )
    .fetch_all(pool)
    .await?;

    let mut enqueued = 0u64;
    for row in rows {
        let nas_ip: Option<String> = row.try_get("nas_ip").ok();
        let router_id = resolve_router_id(pool, nas_ip.as_deref()).await?;
        // Automatic (policy) disconnects use the proven RouterOS management API,
        // never CoA, so a NAS that NAKs Disconnect cannot block expiry handling.
        let method = DisconnectMethod::RouterOs;
        let request = NewDisconnect {
            username: row.try_get("username")?,
            account_id: row.try_get("account_id")?,
            voucher_id: row.try_get("voucher_id")?,
            session_row_id: row.try_get("id")?,
            router_id,
            nas_ip,
            acct_session_id: row.try_get("acct_session_id")?,
            reason: "policy".to_string(),
            method,
            max_attempts: 5,
        };
        if disconnects::enqueue(pool, &request).await.is_ok() {
            enqueued += 1;
        }
    }
    Ok(enqueued)
}

async fn resolve_router_id(
    pool: &MySqlPool,
    nas_ip: Option<&str>,
) -> Result<Option<u64>, sqlx::Error> {
    let Some(nas_ip) = nas_ip else {
        return Ok(None);
    };
    let id: Option<u64> = sqlx::query_scalar(
        "SELECT id FROM tbl_routers WHERE radius_nas_ip = ? OR host = ? LIMIT 1",
    )
    .bind(nas_ip)
    .bind(nas_ip)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

async fn process_disconnects(
    pool: &MySqlPool,
    config: &SchedulerConfig,
) -> Result<String, sqlx::Error> {
    let due = disconnects::list_due(pool, config.disconnect_batch)
        .await
        .map_err(to_sqlx)?;
    let mut success = 0u64;
    let mut retry = 0u64;
    let mut unsupported = 0u64;

    for record in due {
        match disconnects::dispatch(pool, &record, &config.sender).await {
            disconnects::DisconnectOutcome::Success => {
                disconnects::mark_success(pool, record.id)
                    .await
                    .map_err(to_sqlx)?;
                success += 1;
            }
            disconnects::DisconnectOutcome::Retry { error } => {
                let backoff = disconnects::backoff_secs(u64::from(record.attempts) + 1);
                disconnects::mark_retry(pool, record.id, &error, backoff)
                    .await
                    .map_err(to_sqlx)?;
                retry += 1;
            }
            disconnects::DisconnectOutcome::Unsupported { reason } => {
                disconnects::mark_unsupported(pool, record.id, &reason)
                    .await
                    .map_err(to_sqlx)?;
                unsupported += 1;
            }
        }
    }

    Ok(format!(
        "success={success} retry={retry} unsupported={unsupported}"
    ))
}

async fn check_routers(pool: &MySqlPool, config: &SchedulerConfig) -> Result<u64, sqlx::Error> {
    let Some(key) = config.encryption_key.as_ref() else {
        return Ok(0);
    };
    let routers = crate::routers::list_routers(pool, false)
        .await
        .map_err(to_sqlx)?;

    let mut checked = 0u64;
    for router in routers {
        let started = Instant::now();
        let outcome = crate::routers::connect_router(pool, router.id, key).await;
        let latency = started.elapsed().as_millis() as u32;

        let (reachable, version, identity, error) = match outcome {
            Ok(client) => {
                let version = client.get_resources().await.ok().and_then(|r| r.version);
                let identity = client.get_identity().await.ok().map(|i| i.name);
                (true, version, identity, None)
            }
            Err(error) => (false, None, None, Some(error.to_string())),
        };

        let is_reachable = i8::from(reachable);
        sqlx::query(
            "INSERT INTO tbl_router_status \
             (router_id, checked_at, reachable, latency_ms, version, identity, error) \
             VALUES (?, NOW(), ?, ?, ?, ?, ?)",
        )
        .bind(router.id)
        .bind(is_reachable)
        .bind(if reachable { Some(latency) } else { None })
        .bind(version)
        .bind(identity)
        .bind(error)
        .execute(pool)
        .await?;
        checked += 1;
    }
    Ok(checked)
}

async fn sync_nas(pool: &MySqlPool, config: &SchedulerConfig) -> Result<u64, sqlx::Error> {
    let Some(key) = config.encryption_key.as_ref() else {
        return Ok(0);
    };
    let routers = crate::routers::list_routers(pool, false)
        .await
        .map_err(to_sqlx)?;

    let mut synced = 0u64;
    for router in routers {
        let Some(nas_ip) = router.radius_nas_ip.clone() else {
            continue;
        };
        let secret = router_nas_secret(pool, router.id, key).await?;
        let Some(secret) = secret else {
            continue;
        };

        // Upsert by nasname only; never delete operator-managed entries.
        let existing: Option<i32> =
            sqlx::query_scalar("SELECT id FROM nas WHERE nasname = ? LIMIT 1")
                .bind(&nas_ip)
                .fetch_optional(pool)
                .await?;

        if let Some(id) = existing {
            sqlx::query(
                "UPDATE nas SET shortname = ?, secret = ?, type = 'other', \
                    description = 'RWISP router' WHERE id = ?",
            )
            .bind(&router.name)
            .bind(&secret)
            .bind(id)
            .execute(pool)
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO nas (nasname, shortname, type, secret, description) \
                 VALUES (?, ?, 'other', ?, 'RWISP router')",
            )
            .bind(&nas_ip)
            .bind(&router.name)
            .bind(&secret)
            .execute(pool)
            .await?;
        }
        synced += 1;
    }
    Ok(synced)
}

pub async fn router_nas_secret(
    pool: &MySqlPool,
    router_id: u64,
    key: &EncryptionKey,
) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT nas_secret_encrypted FROM tbl_routers WHERE id = ?")
        .bind(router_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let encrypted: Option<Vec<u8>> = row.try_get("nas_secret_encrypted")?;
    match encrypted {
        Some(bytes) => Ok(key.decrypt(&bytes).ok()),
        None => Ok(None),
    }
}

async fn cleanup(pool: &MySqlPool, retention_days: u32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM tbl_router_status WHERE checked_at < DATE_SUB(NOW(), INTERVAL ? DAY)",
    )
    .bind(retention_days.max(1))
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

fn to_sqlx(error: impl std::fmt::Display) -> sqlx::Error {
    sqlx::Error::Protocol(error.to_string())
}

#[derive(Debug, Clone)]
pub struct JobStatus {
    pub job_name: String,
    pub lease_owner: Option<String>,
    pub lease_until: Option<String>,
    pub last_started_at: Option<String>,
    pub last_finished_at: Option<String>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_detail: Option<String>,
    pub runs: u64,
}

pub async fn status(pool: &MySqlPool) -> Result<Vec<JobStatus>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT job_name, lease_owner, CAST(lease_until AS CHAR) AS lease_until, \
            CAST(last_started_at AS CHAR) AS last_started_at, \
            CAST(last_finished_at AS CHAR) AS last_finished_at, last_status, last_error, \
            last_detail, runs \
         FROM tbl_scheduler_state ORDER BY job_name",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(JobStatus {
            job_name: row.try_get("job_name")?,
            lease_owner: row.try_get("lease_owner")?,
            lease_until: row.try_get("lease_until")?,
            last_started_at: row.try_get("last_started_at")?,
            last_finished_at: row.try_get("last_finished_at")?,
            last_status: row.try_get("last_status")?,
            last_error: row.try_get("last_error")?,
            last_detail: row.try_get("last_detail")?,
            runs: row.try_get("runs")?,
        });
    }
    Ok(out)
}
