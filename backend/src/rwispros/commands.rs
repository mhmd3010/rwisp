use mikrotik_rs::{CommandBuilder, Event, ReplyResponse};

use crate::rwispros::connection::RouterClient;
use crate::rwispros::errors::{Result, RouterOsError};
use crate::rwispros::types::{
    ActiveSessionEntry, HotspotActive, HotspotHost, HotspotIpBinding, HotspotProfile,
    HotspotServer, HotspotUser, Identity, RadiusServer, Resources,
};

impl RouterClient {
    pub async fn get_identity(&self) -> Result<Identity> {
        let replies = self.run_read_command("/system/identity/print").await?;
        let reply = first_reply(&replies, "/system/identity/print")?;

        Ok(Identity {
            name: attr(reply, "name").unwrap_or_default(),
            revision: attr(reply, "revision"),
        })
    }

    pub async fn get_resources(&self) -> Result<Resources> {
        let replies = self.run_read_command("/system/resource/print").await?;
        let reply = first_reply(&replies, "/system/resource/print")?;

        Ok(Resources {
            uptime: attr(reply, "uptime"),
            version: attr(reply, "version"),
            board_name: attr(reply, "board-name"),
            architecture_name: attr(reply, "architecture-name"),
            cpu: attr(reply, "cpu"),
            cpu_count: parse_u32(attr(reply, "cpu-count")),
            cpu_frequency: parse_u32(attr(reply, "cpu-frequency")),
            cpu_load: parse_u32(attr(reply, "cpu-load")),
            free_memory: parse_u64(attr(reply, "free-memory")),
            total_memory: parse_u64(attr(reply, "total-memory")),
            free_hdd_space: parse_u64(attr(reply, "free-hdd-space")),
            total_hdd_space: parse_u64(attr(reply, "total-hdd-space")),
        })
    }

    async fn run_read_command(&self, command_path: &str) -> Result<Vec<ReplyResponse>> {
        let command = CommandBuilder::new().command(command_path).build();
        let mut receiver = self.device().send_command(command).await?;
        let mut replies = Vec::new();

        while let Some(event) = receiver.recv().await {
            match event {
                Event::Reply { response, .. } => replies.push(response),
                Event::Done { .. } | Event::Empty { .. } => return Ok(replies),
                Event::Trap { response, .. } => {
                    return Err(RouterOsError::Command {
                        message: response.message,
                    });
                }
                Event::Fatal { reason } => {
                    return Err(RouterOsError::Protocol { message: reason });
                }
            }
        }

        Err(RouterOsError::ConnectionClosed)
    }
}

fn first_reply<'a>(replies: &'a [ReplyResponse], command_path: &str) -> Result<&'a ReplyResponse> {
    replies.first().ok_or_else(|| RouterOsError::Command {
        message: format!("{command_path} returned no data"),
    })
}

impl RouterClient {
    pub async fn get_hotspot_servers(&self) -> Result<Vec<HotspotServer>> {
        let replies = self.run_read_command("/ip/hotspot/print").await?;
        Ok(replies.iter().map(hotspot_server).collect())
    }
    pub async fn get_hotspot_profiles(&self) -> Result<Vec<HotspotProfile>> {
        let replies = self.run_read_command("/ip/hotspot/profile/print").await?;
        Ok(replies.iter().map(hotspot_profile).collect())
    }

    pub async fn get_hotspot_users(&self) -> Result<Vec<HotspotUser>> {
        let replies = self.run_read_command("/ip/hotspot/user/print").await?;
        Ok(replies.iter().map(hotspot_user).collect())
    }

    pub async fn get_hotspot_active(&self) -> Result<Vec<HotspotActive>> {
        let replies = self.run_read_command("/ip/hotspot/active/print").await?;
        Ok(replies.iter().map(hotspot_active).collect())
    }

    pub async fn get_hotspot_ip_bindings(&self) -> Result<Vec<HotspotIpBinding>> {
        let replies = self
            .run_read_command("/ip/hotspot/ip-binding/print")
            .await?;
        Ok(replies.iter().map(hotspot_ip_binding).collect())
    }

    pub async fn get_hotspot_hosts(&self) -> Result<Vec<HotspotHost>> {
        let replies = self.run_read_command("/ip/hotspot/host/print").await?;
        Ok(replies.iter().map(hotspot_host).collect())
    }

    /// Configured RADIUS servers (`/radius/print`), used for provisioning.
    pub async fn get_radius_servers(&self) -> Result<Vec<RadiusServer>> {
        let replies = self.run_read_command("/radius/print").await?;
        Ok(replies
            .iter()
            .map(|reply| RadiusServer {
                address: attr(reply, "address"),
                service: attr(reply, "service"),
                disabled: bool_attr(reply, "disabled"),
            })
            .collect())
    }
}

fn hotspot_server(reply: &ReplyResponse) -> HotspotServer {
    HotspotServer {
        name: attr(reply, "name"),
        interface: attr(reply, "interface"),
        address_pool: attr(reply, "address-pool"),
        profile: attr(reply, "profile"),
        idle_timeout: attr(reply, "idle-timeout"),
        keepalive_timeout: attr(reply, "keepalive-timeout"),
        addresses_per_mac: attr(reply, "addresses-per-mac"),
        disabled: bool_attr(reply, "disabled"),
        comment: attr(reply, "comment"),
    }
}

fn hotspot_profile(reply: &ReplyResponse) -> HotspotProfile {
    HotspotProfile {
        name: attr(reply, "name"),
        hotspot_address: attr(reply, "hotspot-address"),
        dns_name: attr(reply, "dns-name"),
        html_directory: attr(reply, "html-directory"),
        login_by: attr(reply, "login-by"),
        use_radius: bool_attr(reply, "use-radius"),
        shared_users: parse_u64(attr(reply, "shared-users")),
        rate_limit: attr(reply, "rate-limit"),
        http_cookie_lifetime: attr(reply, "http-cookie-lifetime"),
    }
}

fn hotspot_user(reply: &ReplyResponse) -> HotspotUser {
    HotspotUser {
        server: attr(reply, "server"),
        name: attr(reply, "name"),
        profile: attr(reply, "profile"),
        uptime: attr(reply, "uptime"),
        bytes_in: parse_u64(attr(reply, "bytes-in")),
        bytes_out: parse_u64(attr(reply, "bytes-out")),
        packets_in: parse_u64(attr(reply, "packets-in")),
        packets_out: parse_u64(attr(reply, "packets-out")),
        limit_uptime: attr(reply, "limit-uptime"),
        limit_bytes_in: parse_u64(attr(reply, "limit-bytes-in")),
        limit_bytes_out: parse_u64(attr(reply, "limit-bytes-out")),
        mac_address: attr(reply, "mac-address"),
        comment: attr(reply, "comment"),
        disabled: bool_attr(reply, "disabled"),
    }
}

fn hotspot_active(reply: &ReplyResponse) -> HotspotActive {
    HotspotActive {
        server: attr(reply, "server"),
        user: attr(reply, "user"),
        address: attr(reply, "address"),
        mac_address: attr(reply, "mac-address"),
        login_by: attr(reply, "login-by"),
        uptime: attr(reply, "uptime"),
        idle_time: attr(reply, "idle-time"),
        session_time_left: attr(reply, "session-time-left"),
        bytes_in: parse_u64(attr(reply, "bytes-in")),
        bytes_out: parse_u64(attr(reply, "bytes-out")),
        packets_in: parse_u64(attr(reply, "packets-in")),
        packets_out: parse_u64(attr(reply, "packets-out")),
    }
}

fn hotspot_ip_binding(reply: &ReplyResponse) -> HotspotIpBinding {
    HotspotIpBinding {
        server: attr(reply, "server"),
        address: attr(reply, "address"),
        mac_address: attr(reply, "mac-address"),
        to_address: attr(reply, "to-address"),
        binding_type: attr(reply, "type"),
        comment: attr(reply, "comment"),
        disabled: bool_attr(reply, "disabled"),
    }
}

fn hotspot_host(reply: &ReplyResponse) -> HotspotHost {
    HotspotHost {
        server: attr(reply, "server"),
        address: attr(reply, "address"),
        to_address: attr(reply, "to-address"),
        mac_address: attr(reply, "mac-address"),
        uptime: attr(reply, "uptime"),
        idle_time: attr(reply, "idle-time"),
        bytes_in: parse_u64(attr(reply, "bytes-in")),
        bytes_out: parse_u64(attr(reply, "bytes-out")),
        packets_in: parse_u64(attr(reply, "packets-in")),
        packets_out: parse_u64(attr(reply, "packets-out")),
        authorized: bool_attr(reply, "authorized"),
        bypassed: bool_attr(reply, "bypassed"),
    }
}

fn attr(reply: &ReplyResponse, key: &str) -> Option<String> {
    reply.attributes.get(key).cloned().flatten()
}

fn parse_u32(value: Option<String>) -> Option<u32> {
    value.and_then(|value| value.trim().parse().ok())
}

fn parse_u64(value: Option<String>) -> Option<u64> {
    value.and_then(|value| value.trim().parse().ok())
}

fn bool_attr(reply: &ReplyResponse, key: &str) -> bool {
    match reply.attributes.get(key) {
        Some(Some(value)) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "false" | "no" | "0"
        ),
        Some(None) => true,
        None => false,
    }
}

impl RouterClient {
    /// Raw `/ip/hotspot/active/print` replies (for reconciliation and targeting).
    pub async fn get_hotspot_active_raw(&self) -> Result<Vec<ReplyResponse>> {
        self.run_read_command("/ip/hotspot/active/print").await
    }

    /// HotSpot active sessions as structured entries (`.id`, user, MAC, address).
    pub async fn get_hotspot_active_entries(&self) -> Result<Vec<ActiveSessionEntry>> {
        let replies = self.run_read_command("/ip/hotspot/active/print").await?;
        Ok(replies
            .iter()
            .map(|reply| ActiveSessionEntry {
                id: attr(reply, ".id"),
                user: attr(reply, "user"),
                mac: attr(reply, "mac-address"),
                address: attr(reply, "address"),
            })
            .collect())
    }

    /// PPPoE active sessions as structured entries (`.id`, name, caller-id, address).
    pub async fn get_ppp_active_entries(&self) -> Result<Vec<ActiveSessionEntry>> {
        let replies = self.run_read_command("/ppp/active/print").await?;
        Ok(replies
            .iter()
            .map(|reply| ActiveSessionEntry {
                id: attr(reply, ".id"),
                user: attr(reply, "name"),
                mac: attr(reply, "caller-id"),
                address: attr(reply, "address"),
            })
            .collect())
    }

    /// Remove an active HotSpot session by its RouterOS `.id` (documented API).
    pub async fn remove_hotspot_active(&self, id: &str) -> Result<()> {
        let command = CommandBuilder::new()
            .command("/ip/hotspot/active/remove")
            .attribute(".id", Some(id))
            .build();
        self.run_write_command(command).await
    }

    /// Remove an active PPPoE session by its RouterOS `.id` (documented API).
    pub async fn remove_ppp_active(&self, id: &str) -> Result<()> {
        let command = CommandBuilder::new()
            .command("/ppp/active/remove")
            .attribute(".id", Some(id))
            .build();
        self.run_write_command(command).await
    }

    /// Find the active-session `.id` matching a session by MAC, framed IP or user.
    pub async fn find_hotspot_active_id(
        &self,
        mac_address: Option<&str>,
        framed_ip: Option<&str>,
        username: &str,
    ) -> Result<Option<String>> {
        let replies = self.run_read_command("/ip/hotspot/active/print").await?;
        for reply in &replies {
            let mac = attr(reply, "mac-address");
            let address = attr(reply, "address");
            let user = attr(reply, "user");
            let matches = (mac_address.is_some() && mac.as_deref() == mac_address)
                || (framed_ip.is_some() && address.as_deref() == framed_ip)
                || user.as_deref() == Some(username);
            if matches {
                return Ok(attr(reply, ".id"));
            }
        }
        Ok(None)
    }

    /// Add a RADIUS server entry for HotSpot (`/radius/add`, documented v7 API).
    pub async fn add_radius_server(
        &self,
        address: &str,
        secret: &str,
        timeout_secs: u32,
    ) -> Result<()> {
        let command = CommandBuilder::new()
            .command("/radius/add")
            .attribute("address", Some(address))
            .attribute("secret", Some(secret))
            .attribute("service", Some("hotspot"))
            .attribute("timeout", Some(&timeout_secs.to_string()))
            .build();
        self.run_write_command(command).await
    }

    async fn run_write_command(&self, command: mikrotik_rs::Command) -> Result<()> {
        let mut receiver = self.device().send_command(command).await?;
        while let Some(event) = receiver.recv().await {
            match event {
                Event::Done { .. } | Event::Empty { .. } => return Ok(()),
                Event::Reply { .. } => {}
                Event::Trap { response, .. } => {
                    return Err(RouterOsError::Command {
                        message: response.message,
                    });
                }
                Event::Fatal { reason } => {
                    return Err(RouterOsError::Protocol { message: reason });
                }
            }
        }
        Err(RouterOsError::ConnectionClosed)
    }
}

#[cfg(test)]
mod tests {
    use mikrotik_rs::{HashMap, ReplyResponse, Tag};

    use super::{
        bool_attr, hotspot_active, hotspot_host, hotspot_ip_binding, hotspot_server, hotspot_user,
    };

    fn reply(attributes: &[(&str, Option<&str>)]) -> ReplyResponse {
        let mut map = HashMap::new();
        for (key, value) in attributes {
            map.insert((*key).to_string(), value.map(|value| value.to_string()));
        }
        ReplyResponse {
            tag: Tag::new(),
            attributes: map,
            attributes_raw: HashMap::new(),
        }
    }

    #[test]
    fn hotspot_user_maps_counters_without_password() {
        let reply = reply(&[
            ("name", Some("alice")),
            ("password", Some("super-secret")),
            ("profile", Some("default")),
            ("bytes-in", Some("1024")),
            ("bytes-out", Some("2048")),
            ("disabled", Some("no")),
        ]);

        let user = hotspot_user(&reply);
        assert_eq!(user.name.as_deref(), Some("alice"));
        assert_eq!(user.profile.as_deref(), Some("default"));
        assert_eq!(user.bytes_in, Some(1024));
        assert_eq!(user.bytes_out, Some(2048));
        assert!(!user.disabled);
    }

    #[test]
    fn hotspot_active_maps_uptime_and_addresses() {
        let reply = reply(&[
            ("user", Some("alice")),
            ("address", Some("10.5.50.23")),
            ("mac-address", Some("00:11:22:33:44:55")),
            ("uptime", Some("1h2m3s")),
            ("bytes-in", Some("10")),
            ("packets-out", Some("7")),
        ]);

        let active = hotspot_active(&reply);
        assert_eq!(active.user.as_deref(), Some("alice"));
        assert_eq!(active.address.as_deref(), Some("10.5.50.23"));
        assert_eq!(active.mac_address.as_deref(), Some("00:11:22:33:44:55"));
        assert_eq!(active.uptime.as_deref(), Some("1h2m3s"));
        assert_eq!(active.bytes_in, Some(10));
        assert_eq!(active.packets_out, Some(7));
    }

    #[test]
    fn hotspot_server_and_binding_handle_optional_attributes() {
        let server = hotspot_server(&reply(&[
            ("name", Some("hotspot1")),
            ("disabled", Some("yes")),
        ]));
        assert_eq!(server.name.as_deref(), Some("hotspot1"));
        assert_eq!(server.interface, None);
        assert!(server.disabled);

        let binding = hotspot_ip_binding(&reply(&[
            ("mac-address", Some("AA:BB:CC:DD:EE:FF")),
            ("type", Some("bypassed")),
        ]));
        assert_eq!(binding.mac_address.as_deref(), Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(binding.binding_type.as_deref(), Some("bypassed"));
        assert!(!binding.disabled);
    }

    #[test]
    fn hotspot_host_maps_flag_attributes() {
        let reply = reply(&[
            ("mac-address", Some("AA:BB:CC:DD:EE:FF")),
            ("authorized", None),
            ("bypassed", Some("no")),
        ]);

        let host = hotspot_host(&reply);
        assert!(host.authorized);
        assert!(!host.bypassed);
    }

    #[test]
    fn bool_attr_distinguishes_flags_and_values() {
        let reply = reply(&[
            ("flag", None),
            ("yes", Some("yes")),
            ("no", Some("no")),
            ("zero", Some("0")),
        ]);
        assert!(bool_attr(&reply, "flag"));
        assert!(bool_attr(&reply, "yes"));
        assert!(!bool_attr(&reply, "no"));
        assert!(!bool_attr(&reply, "zero"));
        assert!(!bool_attr(&reply, "absent"));
    }
}
