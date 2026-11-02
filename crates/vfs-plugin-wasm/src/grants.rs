//! Explicit per-plugin authority. Runtime execution limits are independent of I/O grants.
use ah_vfs::domain::{DriverError, StorageError, StorageErrorKind};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};
use wasmtime_wasi::sockets::SocketAddrUse;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostGrants {
    pub http: HttpGrants,
    pub tcp: Vec<String>,
    pub fs: Vec<DirectoryGrant>,
    pub env: Vec<String>,
    pub limits: ExecutionLimits,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpGrants {
    pub origins: Vec<String>,
    pub private_addresses: Vec<IpAddr>,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
    pub max_headers: usize,
    pub timeout_ms: u64,
    pub allow_redirects: bool,
}
impl Default for HttpGrants {
    fn default() -> Self {
        Self {
            origins: vec![],
            private_addresses: vec![],
            max_request_bytes: 8 * 1024 * 1024,
            max_response_bytes: 8 * 1024 * 1024,
            max_headers: 64,
            timeout_ms: 20_000,
            allow_redirects: false,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryGrant {
    pub host: PathBuf,
    pub guest: String,
    #[serde(default)]
    pub write: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExecutionLimits {
    pub memory_bytes: usize,
    pub fuel: u64,
    pub timeout_ms: u64,
    pub storage_instances: usize,
    pub control_instances: usize,
}
impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 128 * 1024 * 1024,
            fuel: 2_000_000_000,
            timeout_ms: 30_000,
            storage_instances: 16,
            control_instances: 4,
        }
    }
}
impl ExecutionLimits {
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}
fn invalid(message: &str) -> DriverError {
    StorageError::with_source(
        StorageErrorKind::InvalidRequest,
        std::io::Error::other(message.to_owned()),
    )
    .into()
}
#[derive(Clone)]
pub(crate) struct Destination {
    pub host: String,
    pub port: u16,
    pub literal: Option<IpAddr>,
}
impl Destination {
    pub fn parse(value: &str) -> Result<Self, DriverError> {
        let url = url::Url::parse(&format!("tcp://{value}"))
            .map_err(|_| invalid("invalid TCP host:port grant"))?;
        if url.path() != "" && url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(invalid("invalid TCP host:port grant"));
        }
        let host = url
            .host_str()
            .ok_or_else(|| invalid("missing TCP grant host"))?
            .trim_matches(['[', ']'])
            .to_ascii_lowercase();
        let port = url
            .port()
            .filter(|port| *port != 0)
            .ok_or_else(|| invalid("missing TCP grant port"))?;
        let literal = host.parse().ok();
        Ok(Self {
            host,
            port,
            literal,
        })
    }
}
/// Reject special-purpose ranges even when returned by an explicitly allowed DNS name.
/// A literal IP grant is required to authorize these addresses.
pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || a == 100 && (64..=127).contains(&b)
                || a == 192 && b == 0
                || a == 198 && (b == 18 || b == 19)
                || ip.octets() == [168, 63, 129, 16]
                || a == 198 && b == 51 && c == 100
                || a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] == 0xdb8 || s[1] < 0x200))
        }
    }
}
impl HostGrants {
    pub(crate) fn validate(&mut self) -> Result<(), DriverError> {
        if self.limits.storage_instances > tokio::sync::Semaphore::MAX_PERMITS
            || self.limits.control_instances > tokio::sync::Semaphore::MAX_PERMITS
            || self.limits.memory_bytes == 0
            || self.limits.fuel == 0
            || self.limits.timeout_ms == 0
            || self.limits.storage_instances == 0
            || self.limits.control_instances == 0
            || self.http.max_headers == 0
            || self.http.timeout_ms == 0
            || self.http.max_request_bytes == 0
            || self.http.max_response_bytes == 0
        {
            return Err(invalid("WASI limits must be positive"));
        }
        for origin in &mut self.http.origins {
            let url = url::Url::parse(origin).map_err(|_| invalid("invalid HTTP origin grant"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid(
                    "HTTP grants must be origins without paths or credentials",
                ));
            }
            *origin = url.origin().ascii_serialization();
        }
        for target in &self.tcp {
            Destination::parse(target)?;
        }
        for entry in &self.fs {
            if !entry.host.is_absolute()
                || !entry.guest.starts_with('/')
                || entry.guest.split('/').any(|part| part == "..")
            {
                return Err(invalid(
                    "WASI directory grants require absolute host and guest paths",
                ));
            }
        }
        if self
            .env
            .iter()
            .any(|name| name.is_empty() || name.contains(['=', '\0']))
        {
            return Err(invalid("invalid environment variable grant"));
        }
        Ok(())
    }
    pub(crate) fn capabilities(&self) -> Vec<&'static str> {
        let mut caps = vec![];
        if !self.http.origins.is_empty() {
            caps.push("http");
        }
        if !self.tcp.is_empty() {
            caps.push("tcp");
        }
        if !self.fs.is_empty() {
            caps.push("fs.read");
        }
        if self.fs.iter().any(|grant| grant.write) {
            caps.push("fs.write");
        }
        if !self.env.is_empty() {
            caps.push("env");
        }
        caps
    }
}
#[derive(Clone)]
pub(crate) struct NetworkPolicy {
    pub targets: Vec<Destination>,
}
impl NetworkPolicy {
    pub fn new(grants: &HostGrants) -> Result<Self, DriverError> {
        Ok(Self {
            targets: grants
                .tcp
                .iter()
                .map(|target| Destination::parse(target))
                .collect::<Result<_, _>>()?,
        })
    }
    pub fn allows_name(&self, name: &str) -> bool {
        self.targets.iter().any(|target| {
            target
                .host
                .eq_ignore_ascii_case(name.trim_matches(['[', ']']))
        })
    }
    pub async fn allows(&self, addr: SocketAddr, use_: SocketAddrUse) -> bool {
        match use_ {
            SocketAddrUse::TcpBind => {
                return !self.targets.is_empty() && addr.ip().is_unspecified() && addr.port() == 0;
            }
            SocketAddrUse::TcpConnect => {}
            _ => return false,
        }
        let ip = addr.ip().to_canonical();
        for target in &self.targets {
            if target.port != addr.port() {
                continue;
            }
            if let Some(literal) = target.literal {
                if literal.to_canonical() == ip {
                    return true;
                }
                continue;
            }
            if public_ip(ip)
                && let Ok(mut resolved) =
                    tokio::net::lookup_host((target.host.as_str(), target.port)).await
                && resolved.any(|current| current.ip().to_canonical() == ip)
            {
                return true;
            }
        }
        false
    }
}
