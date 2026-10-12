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
            .field("timeout", &self.timeout)
            .field("radius_nas_ip", &self.radius_nas_ip)
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
            .field("timeout", &self.timeout)
            .field("radius_nas_ip", &self.radius_nas_ip)
            .field("enabled", &self.enabled)
            .finish()
    }
}
