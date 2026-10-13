#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resources {
    pub uptime: Option<String>,
    pub version: Option<String>,
    pub board_name: Option<String>,
    pub architecture_name: Option<String>,
    pub cpu: Option<String>,
    pub cpu_count: Option<u32>,
    pub cpu_frequency: Option<u32>,
    pub cpu_load: Option<u32>,
    pub free_memory: Option<u64>,
    pub total_memory: Option<u64>,
    pub free_hdd_space: Option<u64>,
    pub total_hdd_space: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotspotServer {
    pub name: Option<String>,
    pub interface: Option<String>,
    pub address_pool: Option<String>,
    pub profile: Option<String>,
    pub idle_timeout: Option<String>,
    pub keepalive_timeout: Option<String>,
    pub addresses_per_mac: Option<String>,
    pub disabled: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotspotProfile {
    pub name: Option<String>,
    pub hotspot_address: Option<String>,
    pub dns_name: Option<String>,
    pub html_directory: Option<String>,
    pub login_by: Option<String>,
    pub use_radius: bool,
    pub shared_users: Option<u64>,
    pub rate_limit: Option<String>,
    pub http_cookie_lifetime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotspotUser {
    pub server: Option<String>,
    pub name: Option<String>,
    pub profile: Option<String>,
    pub uptime: Option<String>,
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
    pub packets_in: Option<u64>,
    pub packets_out: Option<u64>,
    pub limit_uptime: Option<String>,
    pub limit_bytes_in: Option<u64>,
    pub limit_bytes_out: Option<u64>,
    pub mac_address: Option<String>,
    pub comment: Option<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotspotActive {
    pub server: Option<String>,
    pub user: Option<String>,
    pub address: Option<String>,
    pub mac_address: Option<String>,
    pub login_by: Option<String>,
    pub uptime: Option<String>,
    pub idle_time: Option<String>,
    pub session_time_left: Option<String>,
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
    pub packets_in: Option<u64>,
    pub packets_out: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotspotIpBinding {
    pub server: Option<String>,
    pub address: Option<String>,
    pub mac_address: Option<String>,
    pub to_address: Option<String>,
    pub binding_type: Option<String>,
    pub comment: Option<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotspotHost {
    pub server: Option<String>,
    pub address: Option<String>,
    pub to_address: Option<String>,
    pub mac_address: Option<String>,
    pub uptime: Option<String>,
    pub idle_time: Option<String>,
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
    pub packets_in: Option<u64>,
    pub packets_out: Option<u64>,
    pub authorized: bool,
    pub bypassed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RadiusServer {
    pub address: Option<String>,
    pub service: Option<String>,
    pub disabled: bool,
}

/// A live session entry on the router (HotSpot or PPPoE active), used for exact
/// match-then-remove disconnects.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveSessionEntry {
    pub id: Option<String>,
    pub user: Option<String>,
    pub mac: Option<String>,
    pub address: Option<String>,
}
