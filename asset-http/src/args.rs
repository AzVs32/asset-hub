use clap::Parser;
use std::path::{Path, PathBuf};

/// HTTP 可执行程序参数。HTTP 行为由 `HttpConfig` 控制，命令行只选择共享配置文档。
#[derive(Debug, Parser)]
#[command(name = "asset-http", version, about = "Run the Asset Hub HTTP service")]
pub struct HttpArgs {
    /// Asset Hub TOML 配置文件。
    #[arg(long)]
    config: Option<PathBuf>,
}

impl HttpArgs {
    pub fn from_cli() -> Self {
        Self::parse()
    }

    pub fn config_path(&self) -> Option<&Path> {
        self.config.as_deref()
    }
}
