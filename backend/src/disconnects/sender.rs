use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use sqlx::{MySqlPool, Row};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::crypto::EncryptionKey;
use crate::disconnects::model::{DisconnectMethod, DisconnectOutcome, DisconnectRecord};
use crate::radius::coa;
use crate::routers;
use crate::rwispros::ActiveSessionEntry;
use crate::rwispros::RouterClient;

/// Configuration for delivering disconnects. `radclient_path` empty disables the
/// CoA method; `coa_secret_fallback` is used when a router has no stored secret.
#[derive(Debug, Clone)]
pub struct SenderConfig {
    pub radclient_path: String,
    pub coa_secret_fallback: Option<String>,
    pub timeout: Duration,
    pub encryption_key: Option<Arc<EncryptionKey>>,
}

impl Default for SenderConfig {
    fn default() -> Self {
        Self {
            radclient_path: "radclient".to_string(),
            coa_secret_fallback: None,
            timeout: Duration::from_secs(5),
            encryption_key: None,
        }
    }
}

/// Bounded exponential backoff (seconds) for attempt N (1-based).
pub fn backoff_secs(attempts: u64) -> u64 {
    let exponent = attempts.saturating_sub(1).min(5);
    (10u64 << exponent).min(300)
}

pub async fn dispatch(
    pool: &MySqlPool,
    record: &DisconnectRecord,
    config: &SenderConfig,
) -> DisconnectOutcome {
    match deliver(pool, record, config).await {
        Ok(outcome) => outcome,
        Err(error) => DisconnectOutcome::Retry {
            error: format!("disconnect delivery error: {error}"),
        },
    }
}

async fn deliver(
    pool: &MySqlPool,
    record: &DisconnectRecord,
    config: &SenderConfig,
) -> Result<DisconnectOutcome, String> {
    // 1) CoA/Disconnect-Request only when this record is assigned the CoA
    //    method AND a reachable endpoint + secret exist. Auto-expiry records use
    //    the RouterOS method, so they never hit a NAS that NAKs Disconnect.
    let try_coa = record.method == DisconnectMethod::Coa;
    let try_routeros = matches!(
        record.method,
        DisconnectMethod::Coa | DisconnectMethod::RouterOs
    );
    if try_coa
        && !config.radclient_path.trim().is_empty()
        && let Some(nas_ip) = record.nas_ip.as_deref()
        && let Some(target) = coa::resolve(pool, nas_ip)
            .await
            .map_err(|e| e.to_string())?
        && target.disconnect_reachable
        && let Some(secret) = resolve_secret(pool, target.router_id, config).await?
    {
        let session_id = record.acct_session_id.clone().unwrap_or_default();
        if !session_id.is_empty() {
            return coa_disconnect(
                &config.radclient_path,
                &target.address,
                target.port,
                &secret,
                &session_id,
                &record.username,
                config.timeout,
            )
            .await;
        }
    }

    // 2) RouterOS management API (the proven mechanism).
    if try_routeros && let Some(router_id) = record.router_id {
        let Some(key) = config.encryption_key.as_ref() else {
            return Ok(DisconnectOutcome::Unsupported {
                reason: "router management fallback requires the credential key".to_string(),
            });
        };
        return routeros_disconnect(pool, router_id, record, key).await;
    }

    // 3) Neither CoA nor a management target is available (e.g. CGNAT with no
    //    disconnect endpoint). Preserve the operation and report the limitation.
    Ok(DisconnectOutcome::Unsupported {
        reason: "no reachable Disconnect/CoA endpoint or management router".to_string(),
    })
}

async fn resolve_secret(
    pool: &MySqlPool,
    router_id: u64,
    config: &SenderConfig,
) -> Result<Option<String>, String> {
    if let Some(key) = config.encryption_key.as_ref() {
        let row = sqlx::query("SELECT coa_secret_encrypted FROM tbl_routers WHERE id = ?")
            .bind(router_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(row) = row {
            let encrypted: Option<Vec<u8>> = row
                .try_get("coa_secret_encrypted")
                .map_err(|e| e.to_string())?;
            if let Some(encrypted) = encrypted {
                let secret = key.decrypt(&encrypted).map_err(|e| e.to_string())?;
                return Ok(Some(secret));
            }
        }
    }
    Ok(config.coa_secret_fallback.clone())
}

async fn coa_disconnect(
    radclient: &str,
    address: &str,
    port: u16,
    secret: &str,
    acct_session_id: &str,
    username: &str,
    timeout: Duration,
) -> Result<DisconnectOutcome, String> {
    let attributes = format!("Acct-Session-Id={acct_session_id},User-Name={username}");

    let mut child = Command::new(radclient)
        .arg("-x")
        .arg(format!("{address}:{port}"))
        .arg("disconnect")
        .arg(secret)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("failed to run radclient: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(attributes.as_bytes())
            .await
            .map_err(|e| format!("failed to write radclient request: {e}"))?;
    }

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(result) => result.map_err(|e| format!("radclient wait failed: {e}"))?,
        Err(_) => {
            return Ok(DisconnectOutcome::Retry {
                error: "Disconnect-Request timed out".to_string(),
            });
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Only an explicit ACK counts as disconnected - never infer success.
    if stdout.contains("Disconnect-ACK") {
        return Ok(DisconnectOutcome::Success);
    }
    if stdout.contains("Disconnect-NAK") {
        return Ok(DisconnectOutcome::Unsupported {
            reason: "NAS returned Disconnect-NAK".to_string(),
        });
    }
    Ok(DisconnectOutcome::Retry {
        error: format!(
            "no Disconnect-ACK received (exit {:?})",
            output.status.code()
        ),
    })
}

async fn routeros_disconnect(
    pool: &MySqlPool,
    router_id: u64,
    record: &DisconnectRecord,
    key: &EncryptionKey,
) -> Result<DisconnectOutcome, String> {
    let (mac, framed_ip) = session_endpoints(pool, record.session_row_id).await?;
    Ok(routeros_disconnect_now(
        pool,
        router_id,
        &record.username,
        mac.as_deref(),
        framed_ip.as_deref(),
        key,
    )
    .await)
}

/// Immediate, exact-match disconnect via the RouterOS management API.
///
/// Tries HotSpot (`/ip/hotspot/active`) then PPPoE (`/ppp/active`); only a
/// single unambiguous entry that matches the username (and MAC/IP when several
/// share the username) is removed, so unrelated users are never disconnected.
pub async fn routeros_disconnect_now(
    pool: &MySqlPool,
    router_id: u64,
    username: &str,
    mac: Option<&str>,
    framed_ip: Option<&str>,
    key: &EncryptionKey,
) -> DisconnectOutcome {
    let client = match routers::connect_router(pool, router_id, key).await {
        Ok(client) => client,
        Err(error) => {
            return DisconnectOutcome::Retry {
                error: format!("router management connection failed: {error}"),
            };
        }
    };

    // HotSpot first.
    match client.get_hotspot_active_entries().await {
        Ok(entries) => match select_entry(&entries, username, mac, framed_ip) {
            Selection::One(id) => return remove_active(&client, ActiveKind::Hotspot, &id).await,
            Selection::Ambiguous => {
                return DisconnectOutcome::Unsupported {
                    reason: "ambiguous HotSpot match; refusing to disconnect unrelated sessions"
                        .to_string(),
                };
            }
            Selection::None => {}
        },
        Err(error) => {
            return DisconnectOutcome::Retry {
                error: format!("HotSpot active lookup failed: {error}"),
            };
        }
    }

    // Then PPPoE.
    match client.get_ppp_active_entries().await {
        Ok(entries) => match select_entry(&entries, username, mac, framed_ip) {
            Selection::One(id) => remove_active(&client, ActiveKind::Pppoe, &id).await,
            Selection::Ambiguous => DisconnectOutcome::Unsupported {
                reason: "ambiguous PPPoE match; refusing to disconnect unrelated sessions"
                    .to_string(),
            },
            // Not present on either service: already offline (idempotent).
            Selection::None => DisconnectOutcome::Success,
        },
        Err(error) => DisconnectOutcome::Retry {
            error: format!("PPPoE active lookup failed: {error}"),
        },
    }
}

enum ActiveKind {
    Hotspot,
    Pppoe,
}

async fn remove_active(client: &RouterClient, kind: ActiveKind, id: &str) -> DisconnectOutcome {
    let result = match kind {
        ActiveKind::Hotspot => client.remove_hotspot_active(id).await,
        ActiveKind::Pppoe => client.remove_ppp_active(id).await,
    };
    match result {
        Ok(()) => DisconnectOutcome::Success,
        Err(error) => DisconnectOutcome::Retry {
            error: format!("RouterOS active-session removal failed: {error}"),
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Selection {
    None,
    One(String),
    Ambiguous,
}

fn select_entry(
    entries: &[ActiveSessionEntry],
    username: &str,
    mac: Option<&str>,
    framed_ip: Option<&str>,
) -> Selection {
    let candidates: Vec<&ActiveSessionEntry> = entries
        .iter()
        .filter(|entry| entry.user.as_deref() == Some(username))
        .collect();

    match candidates.len() {
        0 => Selection::None,
        1 => id_of(candidates[0]),
        _ => {
            if let Some(mac) = mac {
                let wanted = normalize_mac(mac);
                let matches: Vec<&ActiveSessionEntry> = candidates
                    .iter()
                    .copied()
                    .filter(|entry| {
                        entry
                            .mac
                            .as_deref()
                            .map(|value| normalize_mac(value) == wanted)
                            .unwrap_or(false)
                    })
                    .collect();
                if let [only] = matches.as_slice() {
                    return id_of(only);
                }
            }
            if let Some(ip) = framed_ip {
                let matches: Vec<&ActiveSessionEntry> = candidates
                    .iter()
                    .copied()
                    .filter(|entry| entry.address.as_deref() == Some(ip))
                    .collect();
                if let [only] = matches.as_slice() {
                    return id_of(only);
                }
            }
            Selection::Ambiguous
        }
    }
}

fn id_of(entry: &ActiveSessionEntry) -> Selection {
    match entry.id.clone() {
        Some(id) if !id.is_empty() => Selection::One(id),
        _ => Selection::Ambiguous,
    }
}

fn normalize_mac(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase()
}

async fn session_endpoints(
    pool: &MySqlPool,
    session_row_id: Option<u64>,
) -> Result<(Option<String>, Option<String>), String> {
    let Some(session_row_id) = session_row_id else {
        return Ok((None, None));
    };
    let row =
        sqlx::query("SELECT calling_station_id, framed_ip FROM tbl_radius_sessions WHERE id = ?")
            .bind(session_row_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    match row {
        Some(row) => Ok((
            row.try_get("calling_station_id")
                .map_err(|e| e.to_string())?,
            row.try_get("framed_ip").map_err(|e| e.to_string())?,
        )),
        None => Ok((None, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Selection, normalize_mac, select_entry};
    use crate::rwispros::ActiveSessionEntry;

    fn entry(id: &str, user: &str, mac: &str, address: &str) -> ActiveSessionEntry {
        ActiveSessionEntry {
            id: Some(id.to_string()),
            user: Some(user.to_string()),
            mac: Some(mac.to_string()),
            address: Some(address.to_string()),
        }
    }

    #[test]
    fn normalizes_mac() {
        assert_eq!(normalize_mac("AA:BB:CC:DD:EE:FF"), "aabbccddeeff");
        assert_eq!(normalize_mac("aabb.ccdd.eeff"), "aabbccddeeff");
    }

    #[test]
    fn unique_username_matches() {
        let entries = vec![
            entry("*1", "user1", "AA:BB:CC:DD:EE:FF", "10.0.0.5"),
            entry("*2", "other", "11:22:33:44:55:66", "10.0.0.6"),
        ];
        assert_eq!(
            select_entry(&entries, "user1", None, None),
            Selection::One("*1".to_string())
        );
    }

    #[test]
    fn disambiguates_by_mac_and_ip() {
        let entries = vec![
            entry("*1", "user1", "AA:BB:CC:DD:EE:FF", "10.0.0.5"),
            entry("*2", "user1", "11:22:33:44:55:66", "10.0.0.6"),
        ];
        assert_eq!(
            select_entry(&entries, "user1", Some("aa:bb:cc:dd:ee:ff"), None),
            Selection::One("*1".to_string())
        );
        assert_eq!(
            select_entry(&entries, "user1", None, Some("10.0.0.6")),
            Selection::One("*2".to_string())
        );
    }

    #[test]
    fn ambiguous_without_identifier_is_refused() {
        let entries = vec![
            entry("*1", "user1", "", "10.0.0.5"),
            entry("*2", "user1", "", "10.0.0.6"),
        ];
        assert_eq!(
            select_entry(&entries, "user1", None, None),
            Selection::Ambiguous
        );
    }

    #[test]
    fn unknown_user_is_none() {
        let entries = vec![entry("*1", "user1", "AA:BB:CC:DD:EE:FF", "10.0.0.5")];
        assert_eq!(select_entry(&entries, "ghost", None, None), Selection::None);
    }
}
