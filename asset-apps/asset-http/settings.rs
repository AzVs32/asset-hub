use std::env;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8080";
const CONFIG_ENV: &str = "ASSET_HUB_CONFIG";
const ADDR_ENV: &str = "ASSET_HTTP_ADDR";

/// HTTP 应用启动配置。
///
/// 当前只负责读取 HTTP 监听地址和可选配置文件路径。业务和基础设施配置仍由
/// `AssetRuntime` 和 `asset-infra` 处理。
pub(crate) struct HttpSettings {
    addr: SocketAddr,
    config_path: Option<PathBuf>,
}

impl HttpSettings {
    /// 从环境变量读取 HTTP 启动配置。
    ///
    /// - `ASSET_HTTP_ADDR`：监听地址，默认 `127.0.0.1:8080`。
    /// - `ASSET_HUB_CONFIG`：可选 TOML 配置文件路径，未设置时使用默认基础设施配置。
    pub(crate) fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let addr = env::var(ADDR_ENV)
            .unwrap_or_else(|_| DEFAULT_HTTP_ADDR.to_string())
            .parse()?;
        let config_path = env::var_os(CONFIG_ENV).map(PathBuf::from);

        Ok(Self { addr, config_path })
    }

    /// 返回 HTTP 监听地址。
    pub(crate) fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// 返回可选配置文件路径。
    pub(crate) fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }
}
