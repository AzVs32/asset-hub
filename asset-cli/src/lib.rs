use asset_infra::config::AssetInfraConfig;
use asset_runtime::AssetRuntime;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

mod commands;

use commands::{config, plugin, system, user};

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
    /// Manage Asset Hub users.
    User(user::Command),
    /// Seal or verify an installed Asset Hub plugin package.
    Plugin(plugin::Command),
}

pub async fn run(cli: Cli) -> CliResult {
    let config_path = cli.config.as_deref();
    match cli.command {
        Command::Config(command) => config::run(command, config_path),
        Command::System(command) => {
            let runtime = maintenance_runtime(config_path).await?;
            system::run(command, runtime.resource_service()).await
        }
        Command::User(command) => {
            let runtime = maintenance_runtime(config_path).await?;
            user::run(command, runtime.user_service()).await
        }
        Command::Plugin(command) => {
            let config = load_config(config_path)?.normalized()?;
            plugin::run(command, &config.plugin_packages_path())
        }
    }
}

async fn maintenance_runtime(config_path: Option<&Path>) -> CliResult<AssetRuntime> {
    Ok(AssetRuntime::new(load_config(config_path)?).await?)
}

fn load_config(config_path: Option<&Path>) -> Result<AssetInfraConfig, asset_core::CoreError> {
    let config = match config_path {
        Some(path) => AssetInfraConfig::from_config_file(path)?,
        None => AssetInfraConfig::from_default_config_file()?,
    };
    Ok(config)
}

#[cfg(test)]
mod tests;
