use asset_config::{ConfigRegistry, LoadedConfig};
use asset_runtime::{AssetConfig, AssetRuntime};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

mod commands;

use commands::{config, system};

pub type CliResult<T = ()> = anyhow::Result<T>;

#[derive(Debug, Parser)]
#[command(
    name = "asset",
    version,
    about = "Manage Asset Hub from the command line",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Asset Hub TOML configuration file.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect and manage Asset Hub configuration.
    Config(config::Command),
    /// Inspect and maintain the local Asset Hub system.
    System(system::Command),
}

pub async fn run(cli: Cli) -> CliResult {
    let config_path = cli.config.as_deref();
    match cli.command {
        Command::Config(command) => config::run(command, config_path),
        Command::System(command) => {
            let runtime = maintenance_runtime(config_path).await?;
            system::run(command, runtime.storage_maintenance_service()).await
        }
    }
}

async fn maintenance_runtime(config_path: Option<&Path>) -> CliResult<AssetRuntime> {
    let config = load_config(config_path)?;
    let asset_config = config.section::<AssetConfig>()?.clone();
    Ok(AssetRuntime::new(asset_config).await?)
}

pub(crate) fn load_config(config_path: Option<&Path>) -> CliResult<LoadedConfig> {
    Ok(ConfigRegistry::new()
        .with::<AssetConfig>()?
        .load(config_path)?)
}
