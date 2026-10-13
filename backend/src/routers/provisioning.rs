use sqlx::MySqlPool;

use crate::crypto::EncryptionKey;
use crate::routers::error::RouterError;
use crate::routers::repository;

/// A single proposed RouterOS change. Steps are additive/idempotent by design;
/// destructive operations are never generated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionStep {
    pub action: String,
    pub description: String,
    pub command: String,
    pub destructive: bool,
}

/// A human-readable proposed change plan produced by inspecting the router.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProvisionPlan {
    pub router_id: u64,
    pub radius_address: String,
    pub steps: Vec<ProvisionStep>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub applied: Vec<String>,
    pub failed: Vec<String>,
}

/// Inspect the router and produce a plan without changing anything.
pub async fn plan_provisioning(
    pool: &MySqlPool,
    router_id: u64,
    key: &EncryptionKey,
    radius_address: &str,
) -> Result<ProvisionPlan, RouterError> {
    let client = repository::connect_router(pool, router_id, key).await?;

    let radius_servers = client.get_radius_servers().await?;
    let hotspot_servers = client.get_hotspot_servers().await?;
    let profiles = client.get_hotspot_profiles().await?;

    let mut plan = ProvisionPlan {
        router_id,
        radius_address: radius_address.to_string(),
        ..Default::default()
    };

    let already = radius_servers.iter().any(|server| {
        server.address.as_deref() == Some(radius_address)
            && server
                .service
                .as_deref()
                .map(|service| service.contains("hotspot"))
                .unwrap_or(true)
    });

    if already {
        plan.warnings
            .push("RADIUS server for this address is already configured; no change".to_string());
    } else {
        plan.steps.push(ProvisionStep {
            action: "add_radius_server".to_string(),
            description: format!(
                "Add RADIUS server {radius_address} (service=hotspot) for HotSpot authentication and accounting"
            ),
            command: "/radius/add address=<radius> secret=<redacted> service=hotspot".to_string(),
            destructive: false,
        });
    }

    if hotspot_servers.is_empty() {
        plan.warnings
            .push("No HotSpot server is configured on this router".to_string());
    } else {
        for profile in &profiles {
            plan.warnings.push(format!(
                "HotSpot profile '{}' uses login-by={}; verify radius settings manually",
                profile.name.as_deref().unwrap_or("?"),
                profile.login_by.as_deref().unwrap_or("?"),
            ));
        }
    }

    plan.warnings.push(
        "HotSpot profile RADIUS/accounting settings are reported for review only and are never modified automatically"
            .to_string(),
    );

    Ok(plan)
}

/// Apply an approved plan. Requires explicit `approve = true` and never runs
/// destructive commands; partial failures are reported, not hidden.
pub async fn apply_provisioning(
    pool: &MySqlPool,
    router_id: u64,
    key: &EncryptionKey,
    plan: &ProvisionPlan,
    approve: bool,
    radius_address: &str,
    radius_secret: &str,
) -> Result<ApplyReport, RouterError> {
    if !approve {
        return Err(RouterError::Validation {
            message: "provisioning changes require explicit approval".to_string(),
        });
    }
    if radius_secret.is_empty() {
        return Err(RouterError::Validation {
            message: "a RADIUS shared secret is required to provision".to_string(),
        });
    }

    let client = repository::connect_router(pool, router_id, key).await?;
    let mut report = ApplyReport::default();

    for step in &plan.steps {
        match step.action.as_str() {
            "add_radius_server" => {
                // Re-check to avoid duplicate configuration objects.
                let existing = client.get_radius_servers().await?;
                if existing
                    .iter()
                    .any(|server| server.address.as_deref() == Some(radius_address))
                {
                    report
                        .applied
                        .push("add_radius_server skipped: already present".to_string());
                    continue;
                }
                match client
                    .add_radius_server(radius_address, radius_secret, 3)
                    .await
                {
                    Ok(()) => report.applied.push("add_radius_server applied".to_string()),
                    Err(error) => report
                        .failed
                        .push(format!("add_radius_server failed: {error}")),
                }
            }
            other => report
                .failed
                .push(format!("unsupported provisioning action: {other}")),
        }
    }

    Ok(report)
}
