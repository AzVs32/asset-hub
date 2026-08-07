use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::http::HeaderValue;
use clap::{ArgAction, Parser};

const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8080";
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;
const DEFAULT_SESSION_INACTIVITY_SECS: u64 = 12 * 60 * 60;
pub(crate) const DEFAULT_SESSION_SQLITE_PATH: &str = "data/.asset-hub/http-session.sqlite";

#[derive(Debug, Parser)]
#[command(name = "asset-http", version, about = "Run the Asset Hub HTTP service")]
struct HttpCli {
    /// Asset Hub TOML configuration file.
    #[arg(long)]
    config: Option<PathBuf>,

    /// HTTP listen address.
    #[arg(long, default_value = DEFAULT_HTTP_ADDR)]
    addr: SocketAddr,

    /// Enable Swagger UI and the OpenAPI document.
    #[arg(
        long,
        default_value_t = true,
        num_args = 0..=1,
        default_missing_value = "true",
        action = ArgAction::Set
    )]
    enable_swagger: bool,

    /// Enable the permanent resource purge endpoint.
    #[arg(
        long,
        default_value_t = true,
        num_args = 0..=1,
        default_missing_value = "true",
        action = ArgAction::Set
    )]
    enable_purge: bool,

    /// Comma-separated explicit CORS origins.
    #[arg(long, value_parser = parse_cors_policy)]
    cors_allowed_origins: Option<CorsPolicy>,

    /// HTTP request timeout in seconds. Streaming uploads are exempt from the total timeout.
    #[arg(long, default_value_t = DEFAULT_REQUEST_TIMEOUT_SECS)]
    request_timeout_secs: u64,

    /// Mark session cookies as Secure.
    #[arg(
        long,
        default_value_t = false,
        num_args = 0..=1,
        default_missing_value = "true",
        action = ArgAction::Set
    )]
    cookie_secure: bool,

    /// Session inactivity timeout in seconds.
    #[arg(
        long,
        default_value_t = DEFAULT_SESSION_INACTIVITY_SECS,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    session_inactivity_secs: u64,
}

/// HTTP CORS 策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsPolicy {
    /// 不启用 CORS 响应头。
    None,
    /// 只允许显式配置的 origin。
    Origins(Vec<HeaderValue>),
}

/// HTTP 路由边界配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterOptions {
    pub enable_swagger: bool,
    pub enable_purge: bool,
    pub cors: CorsPolicy,
    pub request_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOptions {
    pub cookie_secure: bool,
    pub inactivity_timeout: Duration,
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
/// 负责读取 HTTP 监听、路由和会话边界配置。业务和基础设施配置由外部 host 处理。
/// 未通过命令行 `--config` 指定路径时，由 host 选择默认配置文件。
pub struct HttpSettings {
    addr: SocketAddr,
    config_path: Option<PathBuf>,
    router_options: RouterOptions,
    session_options: SessionOptions,
}

impl HttpSettings {
    /// 使用 Clap 读取命令行参数。
    ///
    /// - `--addr`：监听地址，默认 `127.0.0.1:8080`。
    /// - `--config`：可选配置文件路径，未指定时使用默认 `config.toml`。
    /// - `--enable-swagger`：是否暴露 Swagger UI，默认 `true`。
    /// - `--enable-purge`：是否开放物理删除接口，默认 `true`。
    /// - `--cors-allowed-origins`：逗号分隔的显式 origin，不允许 `*`。
    /// - `--request-timeout-secs`：普通请求总超时秒数，默认 `30`；不限制流式上传总时长。
    /// - `--cookie-secure`：是否为会话 Cookie 添加 Secure，默认 `false`。
    /// - `--session-inactivity-secs`：会话空闲超时秒数，默认 `43200`。
    ///
    /// 布尔选项省略可选值时表示 `true`，显式提供的值只接受 `true` 或 `false`。
    pub fn from_cli() -> Self {
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
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// 返回可选配置文件路径。
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// 返回 HTTP 路由边界配置。
    pub fn router_options(&self) -> &RouterOptions {
        &self.router_options
    }

    pub fn session_options(&self) -> &SessionOptions {
        &self.session_options
    }
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
mod tests;
