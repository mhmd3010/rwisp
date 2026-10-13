use sqlx::{MySqlPool, Row};

use crate::radius::error::RadiusError;

/// A live session that should be disconnected (CoA/Disconnect-Request).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisconnectTarget {
    pub session_id: u64,
    pub username: String,
    pub nas_ip: String,
    pub acct_session_id: String,
}

/// Where a CoA/Disconnect-Request should be sent for a router.
///
/// `address`/`port` are the *disconnect* endpoint, which may differ from the
/// authentication identity (`radius_nas_ip`/`nas_identifier`) and from the
/// management `host`; routers behind NAT/CGNAT that are not reachable are
/// flagged with `disconnect_reachable = false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterCoaTarget {
    pub router_id: u64,
    pub address: String,
    pub port: u16,
    pub disconnect_reachable: bool,
}

const DEFAULT_COA_PORT: u16 = 3799;

pub async fn resolve(
    pool: &MySqlPool,
    nas_ip: &str,
) -> Result<Option<RouterCoaTarget>, RadiusError> {
    let row = sqlx::query(
        "SELECT id, host, coa_address, coa_port, disconnect_reachable \
         FROM tbl_routers WHERE radius_nas_ip = ? OR host = ? LIMIT 1",
    )
    .bind(nas_ip)
    .bind(nas_ip)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let host: String = row.try_get("host")?;
    let coa_address: Option<String> = row.try_get("coa_address")?;
    let coa_port: Option<u16> = row.try_get("coa_port")?;
    let reachable: i8 = row.try_get("disconnect_reachable")?;

    Ok(Some(RouterCoaTarget {
        router_id: row.try_get("id")?,
        address: coa_address
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(host),
        port: coa_port.unwrap_or(DEFAULT_COA_PORT),
        disconnect_reachable: reachable != 0,
    }))
}
