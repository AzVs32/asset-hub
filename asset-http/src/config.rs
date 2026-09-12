use crate::ArchiveOptions;
use asset_config::{ConfigError, ConfigSection};
use axum::http::HeaderValue;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::num::{NonZeroU64, NonZeroUsize};
use std::time::Duration;

const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_REQUEST_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_ARCHIVE_MAX_CONCURRENT: usize = 2;
const DEFAULT_ARCHIVE_MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// HTTP-owned section of the shared application configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    addr: SocketAddr,
    cors_allowed_origins: Vec<String>,
    request_timeout_seconds: u64,
    archive: HttpArchiveConfig,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            addr: DEFAULT_HTTP_ADDR
                .parse()
                .expect("default HTTP address must be valid"),
            cors_allowed_origins: Vec::new(),
            request_timeout_seconds: DEFAULT_REQUEST_TIMEOUT_SECONDS,
            archive: HttpArchiveConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct HttpArchiveConfig {
    max_concurrent: usize,
    max_bytes: u64,
}

impl Default for HttpArchiveConfig {
    fn default() -> Self {
        Self {
            max_concurrent: DEFAULT_ARCHIVE_MAX_CONCURRENT,
            max_bytes: DEFAULT_ARCHIVE_MAX_BYTES,
        }
    }
}

impl ConfigSection for HttpConfig {
    const SECTION: &'static str = "http";

    fn normalize(self) -> Result<Self, String> {
        self.validate()?;
        Ok(self)
    }
}

impl HttpConfig {
    /// 校验 HTTP 配置并一次性转换为启动所需的运行选项。
    pub fn into_runtime_options(self) -> Result<HttpRuntimeOptions, ConfigError> {
        self.validate_limits().map_err(config_error)?;
        let cors = parse_cors_policy(&self.cors_allowed_origins).map_err(config_error)?;
        Ok(HttpRuntimeOptions {
            addr: self.addr,
            archive: ArchiveOptions {
                max_concurrent: NonZeroUsize::new(self.archive.max_concurrent)
                    .expect("validation rejects zero archive concurrency"),
                max_bytes: NonZeroU64::new(self.archive.max_bytes)
                    .expect("validation rejects a zero archive byte limit"),
            },
            router: RouterOptions {
                cors,
                request_timeout: Duration::from_secs(self.request_timeout_seconds),
            },
        })
    }

    fn validate(&self) -> Result<(), String> {
        self.validate_limits()?;
        parse_cors_policy(&self.cors_allowed_origins).map(|_| ())
    }

    fn validate_limits(&self) -> Result<(), String> {
        if self.request_timeout_seconds == 0 {
            return Err("request_timeout_seconds must be greater than 0".to_owned());
        }
        if self.archive.max_concurrent == 0 {
            return Err("archive.max_concurrent must be greater than 0".to_owned());
        }
        if self.archive.max_bytes == 0 {
            return Err("archive.max_bytes must be greater than 0".to_owned());
        }
        Ok(())
    }
}

/// HTTP 可执行程序启动时使用的已校验运行选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRuntimeOptions {
    pub addr: SocketAddr,
    pub archive: ArchiveOptions,
    pub router: RouterOptions,
}

/// HTTP CORS policy used by router assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsPolicy {
    None,
    Origins(Vec<HeaderValue>),
}

/// HTTP router boundary settings derived from [`HttpConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterOptions {
    pub cors: CorsPolicy,
    pub request_timeout: Duration,
}

impl Default for RouterOptions {
    fn default() -> Self {
        HttpConfig::default()
            .into_runtime_options()
            .map(|options| options.router)
            .expect("default HTTP configuration must be valid")
    }
}

fn config_error(reason: String) -> ConfigError {
    ConfigError::Validate {
        section: HttpConfig::SECTION.to_owned(),
        reason,
    }
}

fn parse_cors_policy(origins: &[String]) -> Result<CorsPolicy, String> {
    if origins.is_empty() {
        return Ok(CorsPolicy::None);
    }

    let origins = origins
        .iter()
        .map(|origin| {
            let origin = origin.trim();
            if origin == "*" {
                return Err("wildcard CORS is not supported; configure explicit origins".to_owned());
            }
            if origin.is_empty() {
                return Err("CORS origins must not be empty".to_owned());
            }
            HeaderValue::from_str(origin)
                .map_err(|error| format!("invalid CORS origin `{origin}`: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CorsPolicy::Origins(origins))
}

#[cfg(test)]
mod tests;
