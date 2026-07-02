use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use asset_apps::AssetRuntime;
use axum::http::HeaderValue;

const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8080";
const CONFIG_ENV: &str = "ASSET_HUB_CONFIG";
const ADDR_ENV: &str = "ASSET_HTTP_ADDR";
const ENABLE_SWAGGER_ENV: &str = "ASSET_HTTP_ENABLE_SWAGGER";
const ENABLE_PURGE_ENV: &str = "ASSET_HTTP_ENABLE_PURGE";
const CORS_ALLOWED_ORIGINS_ENV: &str = "ASSET_HTTP_CORS_ALLOWED_ORIGINS";
const REQUEST_TIMEOUT_SECS_ENV: &str = "ASSET_HTTP_REQUEST_TIMEOUT_SECS";
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;

/// HTTP CORS 策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CorsPolicy {
    /// 不启用 CORS 响应头。
    None,
    /// 允许任意 origin，适合本地或受控内网调试。
    Any,
    /// 只允许显式配置的 origin。
    Origins(Vec<HeaderValue>),
}

/// HTTP 路由边界配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouterOptions {
    pub(crate) enable_swagger: bool,
    pub(crate) enable_purge: bool,
    pub(crate) cors: CorsPolicy,
    pub(crate) request_timeout: Duration,
}

impl Default for RouterOptions {
    fn default() -> Self {
        Self {
            enable_swagger: true,
            enable_purge: true,
            cors: CorsPolicy::None,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
        }
    }
}

/// HTTP 应用启动配置。
///
/// 当前只负责读取 HTTP 监听地址和可选配置文件路径。业务和基础设施配置由
/// `AssetRuntime` 和 `asset-infra` 处理。未通过 `ASSET_HUB_CONFIG` 指定路径时，
/// runtime 会尝试读取默认 `config.toml`。
pub(crate) struct HttpSettings {
    addr: SocketAddr,
    config_path: Option<PathBuf>,
    router_options: RouterOptions,
}

impl HttpSettings {
    /// 从环境变量读取 HTTP 启动配置。
    ///
    /// - `ASSET_HTTP_ADDR`：监听地址，默认 `127.0.0.1:8080`。
    /// - `ASSET_HUB_CONFIG`：可选配置文件路径，未设置时使用默认 `config.toml`。
    /// - `ASSET_HTTP_ENABLE_SWAGGER`：是否暴露 Swagger UI，默认 `true`。
    /// - `ASSET_HTTP_ENABLE_PURGE`：是否开放物理删除接口，默认 `true`。
    /// - `ASSET_HTTP_CORS_ALLOWED_ORIGINS`：逗号分隔 origin，`*` 表示全部。
    /// - `ASSET_HTTP_REQUEST_TIMEOUT_SECS`：请求超时秒数，默认 `30`。
    pub(crate) fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let addr = env::var(ADDR_ENV)
            .unwrap_or_else(|_| DEFAULT_HTTP_ADDR.to_string())
            .parse()?;
        let config_path = env::var_os(CONFIG_ENV).map(PathBuf::from);
        let router_options = RouterOptions {
            enable_swagger: parse_bool_env(ENABLE_SWAGGER_ENV, true)?,
            enable_purge: parse_bool_env(ENABLE_PURGE_ENV, true)?,
            cors: parse_cors_policy(env::var(CORS_ALLOWED_ORIGINS_ENV).ok())?,
            request_timeout: Duration::from_secs(parse_u64_env(
                REQUEST_TIMEOUT_SECS_ENV,
                DEFAULT_REQUEST_TIMEOUT_SECS,
            )?),
        };

        Ok(Self {
            addr,
            config_path,
            router_options,
        })
    }

    /// 返回 HTTP 监听地址。
    pub(crate) fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// 返回可选配置文件路径。
    pub(crate) fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// 返回 HTTP 路由边界配置。
    pub(crate) fn router_options(&self) -> &RouterOptions {
        &self.router_options
    }

    /// 返回当前生效的默认配置文件名。
    pub(crate) fn default_config_file(&self) -> &'static str {
        AssetRuntime::default_config_file()
    }
}

fn parse_bool_env(name: &str, default: bool) -> Result<bool, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => parse_bool_value(name, &value),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(Box::new(error)),
    }
}

fn parse_bool_value(name: &str, value: &str) -> Result<bool, Box<dyn std::error::Error>> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("{name} must be a boolean value").into()),
    }
}

fn parse_u64_env(name: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .map_err(|error| format!("{name} must be an integer: {error}").into()),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(Box::new(error)),
    }
}

fn parse_cors_policy(value: Option<String>) -> Result<CorsPolicy, Box<dyn std::error::Error>> {
    let Some(value) = value else {
        return Ok(CorsPolicy::None);
    };
    let origins = value
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .collect::<Vec<_>>();
    if origins.is_empty() {
        return Ok(CorsPolicy::None);
    }
    if origins.iter().any(|origin| *origin == "*") {
        return Ok(CorsPolicy::Any);
    }

    let origins = origins
        .into_iter()
        .map(HeaderValue::from_str)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CorsPolicy::Origins(origins))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_boolean_environment_values() {
        assert!(parse_bool_value("TEST", "true").unwrap());
        assert!(parse_bool_value("TEST", "1").unwrap());
        assert!(!parse_bool_value("TEST", "off").unwrap());
        assert!(parse_bool_value("TEST", "maybe").is_err());
    }

    #[test]
    fn parses_cors_policy() {
        assert_eq!(parse_cors_policy(None).unwrap(), CorsPolicy::None);
        assert_eq!(
            parse_cors_policy(Some("*".to_string())).unwrap(),
            CorsPolicy::Any
        );
        assert_eq!(
            parse_cors_policy(Some(
                "http://127.0.0.1:5173, https://example.com".to_string()
            ))
            .unwrap(),
            CorsPolicy::Origins(vec![
                HeaderValue::from_static("http://127.0.0.1:5173"),
                HeaderValue::from_static("https://example.com"),
            ])
        );
    }
}
