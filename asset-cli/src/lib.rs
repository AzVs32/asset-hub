use asset_infra::config::AssetInfraConfig;
use asset_runtime::AssetRuntime;
use clap::{Parser, Subcommand};

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
    /// Seal and verify Asset Hub plugin packages.
    Plugin(plugin::PluginCommand),
}

pub async fn run(cli: Cli) -> CliResult {
    match cli.command {
        Command::Config(command) => config::run(command),
        Command::System(command) => {
            let runtime = maintenance_runtime().await?;
            system::run(command, runtime.resource_service()).await
        }
        Command::User(command) => {
            let runtime = maintenance_runtime().await?;
            user::run(
                command,
                runtime.user_service(),
                runtime.security_audit_repository(),
            )
            .await
        }
        Command::Plugin(command) => plugin::run(command),
    }
}

async fn maintenance_runtime() -> CliResult<AssetRuntime> {
    Ok(AssetRuntime::new(AssetInfraConfig::from_default_config_file()?).await?)
}

#[cfg(test)]
mod tests;
