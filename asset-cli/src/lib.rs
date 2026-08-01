use asset_infra::config::AssetInfraConfig;
use asset_runtime::AssetRuntime;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

mod audit;
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
    Config(config::ConfigCommand),
    /// Inspect and maintain the local Asset Hub system.
    System(system::SystemCommand),
    /// Manage Asset Hub users.
    User(user::UserCommand),
    /// Verify Asset Hub plugin packages.
    Plugin(plugin::PluginCommand),
}

pub async fn run(cli: Cli) -> CliResult {
    let config_path = cli.config.as_deref();
    match cli.command {
        Command::Config(command) => config::run(command, config_path),
        Command::System(command) => {
            let runtime = maintenance_runtime(config_path).await?;
            system::run(
                command,
                runtime.resource_service(),
                runtime.security_audit_repository(),
            )
            .await
        }
        Command::User(command) => {
            let runtime = maintenance_runtime(config_path).await?;
            user::run(
                command,
                runtime.user_service(),
                runtime.security_audit_repository(),
            )
            .await
        }
        Command::Plugin(command) => {
            if config_path.is_some() {
                anyhow::bail!("--config is not used by `asset plugin`");
            }
            plugin::run(command)
        }
    }
}

async fn maintenance_runtime(config_path: Option<&Path>) -> CliResult<AssetRuntime> {
    let config = match config_path {
        Some(path) => AssetInfraConfig::from_config_file(path)?,
        None => AssetInfraConfig::from_default_config_file()?,
    };
    Ok(AssetRuntime::new(config).await?)
}

#[cfg(test)]
mod tests;
