use std::fmt;
use std::time::Duration;

use crate::rwispros::Transport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterRecord {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub api_port: u16,
    pub username: String,
    pub transport: Transport,
    pub timeout: Duration,
    pub radius_nas_ip: Option<String>,
    pub nas_identifier: Option<String>,
    pub coa_address: Option<String>,
    pub coa_port: Option<u16>,
    pub auth_reachable: bool,
    pub disconnect_reachable: bool,
    pub has_coa_secret: bool,
    pub has_nas_secret: bool,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub struct NewRouter {
    pub name: String,
    pub host: String,
    pub api_port: u16,
    pub username: String,
    password: String,
    pub transport: Transport,
    pub timeout: Duration,
    pub radius_nas_ip: Option<String>,
    pub nas_identifier: Option<String>,
    pub coa_address: Option<String>,
    pub coa_port: Option<u16>,
    pub auth_reachable: bool,
    pub disconnect_reachable: bool,
    pub enabled: bool,
}

impl NewRouter {
    pub fn new(
        name: impl Into<String>,
        host: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        let transport = Transport::Tcp;
        Self {
            name: name.into(),
            host: host.into(),
            api_port: transport.default_port(),
            username: username.into(),
            password: password.into(),
            transport,
            timeout: Duration::from_secs(10),
            radius_nas_ip: None,
            nas_identifier: None,
            coa_address: None,
            coa_port: None,
            auth_reachable: true,
            disconnect_reachable: true,
            enabled: true,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.api_port = port;
        self
    }

    pub fn with_transport(mut self, transport: Transport) -> Self {
        self.transport = transport;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_radius_nas_ip(mut self, radius_nas_ip: impl Into<String>) -> Self {
        self.radius_nas_ip = Some(radius_nas_ip.into());
        self
    }

    pub fn with_nas_identifier(mut self, value: impl Into<String>) -> Self {
        self.nas_identifier = Some(value.into());
        self
    }

    pub fn with_coa(mut self, address: impl Into<String>, port: u16) -> Self {
        self.coa_address = Some(address.into());
        self.coa_port = Some(port);
        self.disconnect_reachable = true;
        self
    }

    pub fn with_disconnect_reachable(mut self, value: bool) -> Self {
        self.disconnect_reachable = value;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub(crate) fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for NewRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewRouter")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("api_port", &self.api_port)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("transport", &self.transport)
            .field("radius_nas_ip", &self.radius_nas_ip)
            .field("nas_identifier", &self.nas_identifier)
            .field("coa_address", &self.coa_address)
            .field("coa_port", &self.coa_port)
            .field("auth_reachable", &self.auth_reachable)
            .field("disconnect_reachable", &self.disconnect_reachable)
            .field("enabled", &self.enabled)
            .finish()
    }
}

pub struct RouterUpdate {
    pub name: String,
    pub host: String,
    pub api_port: u16,
    pub username: String,
    password: Option<String>,
    pub transport: Transport,
    pub timeout: Duration,
    pub radius_nas_ip: Option<String>,
    pub nas_identifier: Option<String>,
    pub coa_address: Option<String>,
    pub coa_port: Option<u16>,
    pub auth_reachable: bool,
    pub disconnect_reachable: bool,
    pub enabled: bool,
}

impl RouterUpdate {
    pub fn from_record(record: &RouterRecord) -> Self {
        Self {
            name: record.name.clone(),
            host: record.host.clone(),
            api_port: record.api_port,
            username: record.username.clone(),
            password: None,
            transport: record.transport,
            timeout: record.timeout,
            radius_nas_ip: record.radius_nas_ip.clone(),
            nas_identifier: record.nas_identifier.clone(),
            coa_address: record.coa_address.clone(),
            coa_port: record.coa_port,
            auth_reachable: record.auth_reachable,
            disconnect_reachable: record.disconnect_reachable,
            enabled: record.enabled,
        }
    }

    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    pub(crate) fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }
}

impl fmt::Debug for RouterUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouterUpdate")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("api_port", &self.api_port)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("transport", &self.transport)
            .field("radius_nas_ip", &self.radius_nas_ip)
            .field("nas_identifier", &self.nas_identifier)
            .field("coa_address", &self.coa_address)
            .field("coa_port", &self.coa_port)
            .field("auth_reachable", &self.auth_reachable)
            .field("disconnect_reachable", &self.disconnect_reachable)
            .field("enabled", &self.enabled)
            .finish()
    }
}
