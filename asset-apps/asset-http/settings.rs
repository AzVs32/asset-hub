use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use asset_apps::AssetRuntime;
use axum::http::HeaderValue;
use clap::{ArgAction, Parser};

const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8080";
const CONFIG_ENV: &str = "ASSET_HUB_CONFIG";
const ADDR_ENV: &str = "ASSET_HTTP_ADDR";
const ENABLE_SWAGGER_ENV: &str = "ASSET_HTTP_ENABLE_SWAGGER";
const ENABLE_PURGE_ENV: &str = "ASSET_HTTP_ENABLE_PURGE";
const CORS_ALLOWED_ORIGINS_ENV: &str = "ASSET_HTTP_CORS_ALLOWED_ORIGINS";
const REQUEST_TIMEOUT_SECS_ENV: &str = "ASSET_HTTP_REQUEST_TIMEOUT_SECS";
const COOKIE_SECURE_ENV: &str = "ASSET_HTTP_COOKIE_SECURE";
const SESSION_INACTIVITY_SECS_ENV: &str = "ASSET_HTTP_SESSION_INACTIVITY_SECS";
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
const DEFAULT_SESSION_INACTIVITY_SECS: u64 = 12 * 60 * 60;

#[derive(Debug, Parser)]
#[command(name = "asset-http", version, about = "Run the Asset Hub HTTP service")]
struct HttpCli {
    /// Asset Hub TOML configuration file.
    #[arg(long, env = CONFIG_ENV)]
    config: Option<PathBuf>,

    /// HTTP listen address.
    #[arg(long, env = ADDR_ENV, default_value = DEFAULT_HTTP_ADDR)]
    addr: SocketAddr,

    /// Enable Swagger UI and the OpenAPI document.
    #[arg(
        long,
        env = ENABLE_SWAGGER_ENV,
        default_value_t = true,
        value_parser = parse_bool_value,
        num_args = 0..=1,
        default_missing_value = "true",
        action = ArgAction::Set
    )]
    enable_swagger: bool,

    /// Enable the permanent resource purge endpoint.
    #[arg(
        long,
        env = ENABLE_PURGE_ENV,
        default_value_t = true,
        value_parser = parse_bool_value,
        num_args = 0..=1,
        default_missing_value = "true",
        action = ArgAction::Set
    )]
    enable_purge: bool,

    /// Comma-separated explicit CORS origins.
    #[arg(long, env = CORS_ALLOWED_ORIGINS_ENV, value_parser = parse_cors_policy)]
    cors_allowed_origins: Option<CorsPolicy>,

    /// HTTP request timeout in seconds.
    #[arg(long, env = REQUEST_TIMEOUT_SECS_ENV, default_value_t = DEFAULT_REQUEST_TIMEOUT_SECS)]
    request_timeout_secs: u64,

    /// Mark session cookies as Secure.
    #[arg(
        long,
        env = COOKIE_SECURE_ENV,
        default_value_t = false,
        value_parser = parse_bool_value,
        num_args = 0..=1,
        default_missing_value = "true",
        action = ArgAction::Set
    )]
    cookie_secure: bool,

    /// Session inactivity timeout in seconds.
    #[arg(
        long,
        env = SESSION_INACTIVITY_SECS_ENV,
        default_value_t = DEFAULT_SESSION_INACTIVITY_SECS,
        value_parser = parse_positive_u64_value
    )]
    session_inactivity_secs: u64,
}

/// HTTP CORS 策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CorsPolicy {
    /// 不启用 CORS 响应头。
    None,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionOptions {
    pub(crate) cookie_secure: bool,
    pub(crate) inactivity_timeout: Duration,
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
/// 负责读取 HTTP 监听、路由和会话边界配置。业务和基础设施配置由 `AssetRuntime`
/// 和 `asset-infra` 处理。未通过命令行或 `ASSET_HUB_CONFIG` 指定路径时，runtime
/// 会尝试读取默认 `config.toml`。
pub(crate) struct HttpSettings {
    addr: SocketAddr,
    config_path: Option<PathBuf>,
    router_options: RouterOptions,
    session_options: SessionOptions,
}

impl HttpSettings {
    /// 使用 Clap 读取命令行参数和兼容的环境变量。
    ///
    /// - `ASSET_HTTP_ADDR`：监听地址，默认 `127.0.0.1:8080`。
    /// - `ASSET_HUB_CONFIG`：可选配置文件路径，未设置时使用默认 `config.toml`。
    /// - `ASSET_HTTP_ENABLE_SWAGGER`：是否暴露 Swagger UI，默认 `true`。
    /// - `ASSET_HTTP_ENABLE_PURGE`：是否开放物理删除接口，默认 `true`。
    /// - `ASSET_HTTP_CORS_ALLOWED_ORIGINS`：逗号分隔的显式 origin，不允许 `*`。
    /// - `ASSET_HTTP_REQUEST_TIMEOUT_SECS`：请求超时秒数，默认 `30`。
    /// - `ASSET_HTTP_COOKIE_SECURE`：是否为会话 Cookie 添加 Secure，默认 `false`。
    /// - `ASSET_HTTP_SESSION_INACTIVITY_SECS`：会话空闲超时秒数，默认 `43200`。
    pub(crate) fn from_cli() -> Self {
        Self::from_cli_args(HttpCli::parse())
    }

    fn from_cli_args(cli: HttpCli) -> Self {
        let router_options = RouterOptions {
            enable_swagger: cli.enable_swagger,
            enable_purge: cli.enable_purge,
            cors: cli.cors_allowed_origins.unwrap_or(CorsPolicy::None),
            request_timeout: Duration::from_secs(cli.request_timeout_secs),
        };
        let session_options = SessionOptions {
            cookie_secure: cli.cookie_secure,
            inactivity_timeout: Duration::from_secs(cli.session_inactivity_secs),
        };

        Self {
            addr: cli.addr,
            config_path: cli.config,
            router_options,
            session_options,
        }
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

    pub(crate) fn session_options(&self) -> &SessionOptions {
        &self.session_options
    }

    /// 返回当前生效的默认配置文件名。
    pub(crate) fn default_config_file(&self) -> &'static str {
        AssetRuntime::default_config_file()
    }
}

fn parse_bool_value(value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err("expected a boolean value (true/false, yes/no, on/off, or 1/0)".to_string()),
    }
}

fn parse_positive_u64_value(value: &str) -> Result<u64, String> {
    let value = value
        .trim()
        .parse::<u64>()
        .map_err(|error| format!("expected a positive integer: {error}"))?;
    if value == 0 {
        return Err("value must be greater than zero".to_string());
    }
    Ok(value)
}

fn parse_cors_policy(value: &str) -> Result<CorsPolicy, String> {
    let origins = value
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .collect::<Vec<_>>();
    if origins.is_empty() {
        return Ok(CorsPolicy::None);
    }
    if origins.contains(&"*") {
        return Err(
            "wildcard CORS is not supported with cookie authentication; configure explicit origins"
                .to_string(),
        );
    }

    let origins = origins
        .into_iter()
        .map(|origin| {
            HeaderValue::from_str(origin)
                .map_err(|error| format!("invalid CORS origin `{origin}`: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CorsPolicy::Origins(origins))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_boolean_environment_values() {
        assert!(parse_bool_value("true").unwrap());
        assert!(parse_bool_value("1").unwrap());
        assert!(!parse_bool_value("off").unwrap());
        assert!(parse_bool_value("maybe").is_err());
    }

    #[test]
    fn clap_parses_http_flags_into_settings() {
        let cli = HttpCli::try_parse_from([
            "asset-http",
            "--config",
            "custom.toml",
            "--addr",
            "0.0.0.0:9000",
            "--enable-swagger=false",
            "--enable-purge=false",
            "--cors-allowed-origins",
            "https://example.com",
            "--request-timeout-secs",
            "45",
            "--cookie-secure",
            "--session-inactivity-secs",
            "3600",
        ])
        .unwrap();
        let settings = HttpSettings::from_cli_args(cli);

        assert_eq!(settings.addr, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(settings.config_path, Some(PathBuf::from("custom.toml")));
        assert!(!settings.router_options.enable_swagger);
        assert!(!settings.router_options.enable_purge);
        assert_eq!(
            settings.router_options.request_timeout,
            Duration::from_secs(45)
        );
        assert!(settings.session_options.cookie_secure);
        assert_eq!(
            settings.session_options.inactivity_timeout,
            Duration::from_secs(3600)
        );
    }

    #[test]
    fn clap_rejects_invalid_http_boundaries() {
        assert!(HttpCli::try_parse_from(["asset-http", "--session-inactivity-secs", "0"]).is_err());
        assert!(HttpCli::try_parse_from(["asset-http", "--cors-allowed-origins", "*"]).is_err());
    }

    #[test]
    fn parses_cors_policy() {
        assert_eq!(parse_cors_policy("").unwrap(), CorsPolicy::None);
        assert!(parse_cors_policy("*").is_err());
        assert_eq!(
            parse_cors_policy("http://127.0.0.1:5173, https://example.com").unwrap(),
            CorsPolicy::Origins(vec![
                HeaderValue::from_static("http://127.0.0.1:5173"),
                HeaderValue::from_static("https://example.com"),
            ])
        );
    }
}
