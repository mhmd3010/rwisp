use std::fmt;
use std::future::Future;
use std::net::IpAddr as StdIpAddr;
use std::sync::Arc;
use std::time::Duration;

use mikrotik_rs::{DeviceError, MikrotikDevice};
use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::pki_types::ServerName;

use crate::rwispros::errors::{Result, RouterOsError};

const DEFAULT_PORT: u16 = 8728;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Tls,
    TlsInsecure,
}

impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Tls => "tls",
            Self::TlsInsecure => "tls_insecure",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tcp" => Some(Self::Tcp),
            "tls" => Some(Self::Tls),
            "tls_insecure" | "tls-insecure" => Some(Self::TlsInsecure),
            _ => None,
        }
    }

    pub fn default_port(&self) -> u16 {
        match self {
            Self::Tcp => DEFAULT_PORT,
            Self::Tls | Self::TlsInsecure => 8729,
        }
    }
}

pub struct RouterConfig {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
    pub transport: Transport,
    username: String,
    password: String,
}

impl RouterConfig {
    pub fn new(
        host: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port: DEFAULT_PORT,
            timeout: DEFAULT_TIMEOUT,
            transport: Transport::Tcp,
            username: username.into(),
            password: password.into(),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_transport(mut self, transport: Transport) -> Self {
        self.transport = transport;
        self
    }

    pub(crate) fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub(crate) fn username(&self) -> &str {
        &self.username
    }

    pub(crate) fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for RouterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouterConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("timeout", &self.timeout)
            .field("transport", &self.transport)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct RouterClient {
    device: MikrotikDevice,
    host: String,
}

impl RouterClient {
    pub async fn connect(config: RouterConfig) -> Result<Self> {
        let address = config.address();
        let host = config.host.clone();
        let timeout = config.timeout;

        let device = match config.transport {
            Transport::Tcp => {
                let builder = MikrotikDevice::builder(address.as_str())
                    .credentials(config.username(), Some(config.password()));
                connect_with_timeout(timeout, &host, builder.connect()).await?
            }
            Transport::Tls => {
                let (tls_config, server_name) = validated_tls_config(&host)?;
                let builder = MikrotikDevice::builder(address.as_str())
                    .credentials(config.username(), Some(config.password()))
                    .tls_config(tls_config, server_name);
                connect_with_timeout(timeout, &host, builder.connect()).await?
            }
            Transport::TlsInsecure => {
                let builder = MikrotikDevice::builder(address.as_str())
                    .credentials(config.username(), Some(config.password()))
                    .tls_insecure();
                connect_with_timeout(timeout, &host, builder.connect()).await?
            }
        };

        Ok(Self { device, host })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn device(&self) -> &MikrotikDevice {
        &self.device
    }
}

async fn connect_with_timeout<F>(
    timeout: Duration,
    host: &str,
    connect: F,
) -> Result<MikrotikDevice>
where
    F: Future<Output = std::result::Result<MikrotikDevice, DeviceError>>,
{
    match tokio::time::timeout(timeout, connect).await {
        Ok(Ok(device)) => Ok(device),
        Ok(Err(error)) => Err(map_connect_error(error, host)),
        Err(_) => Err(RouterOsError::Timeout {
            host: host.to_string(),
            timeout,
        }),
    }
}

fn map_connect_error(error: DeviceError, host: &str) -> RouterOsError {
    match error {
        DeviceError::Connection(source) => RouterOsError::Unreachable {
            host: host.to_string(),
            source,
        },
        other => other.into(),
    }
}

fn validated_tls_config(host: &str) -> Result<(Arc<ClientConfig>, ServerName<'static>)> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if roots.is_empty() {
        return Err(RouterOsError::Tls {
            message: "no trusted root certificates available".to_string(),
        });
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| RouterOsError::Tls {
            message: error.to_string(),
        })?
        .with_root_certificates(roots)
        .with_no_client_auth();

    let server_name = parse_server_name(host)?;
    Ok((Arc::new(config), server_name))
}

fn parse_server_name(host: &str) -> Result<ServerName<'static>> {
    if let Ok(ip) = host.parse::<StdIpAddr>() {
        return Ok(ServerName::IpAddress(ip.into()));
    }
    ServerName::try_from(host.to_string()).map_err(|error| RouterOsError::Tls {
        message: format!("invalid server name {host}: {error}"),
    })
}
