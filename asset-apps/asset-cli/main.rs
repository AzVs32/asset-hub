use clap::{Parser, Subcommand};

mod commands;

use commands::{CliResult, config, plugin, system, user};

#[derive(Debug, Parser)]
#[command(
    name = "asset",
    version,
    about = "Manage Asset Hub from the command line",
    arg_required_else_help = true
)]
struct Cli {
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
    /// Build and manage Asset Hub plugins.
    Plugin(plugin::PluginCommand),
}

#[tokio::main]
async fn main() {
    if let Err(error) = run(Cli::parse()).await {
        eprintln!("asset: {error}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> CliResult {
    match cli.command {
        Command::Config(command) => config::run(command),
        Command::System(command) => system::run(command).await,
        Command::User(command) => user::run(command).await,
        Command::Plugin(command) => plugin::run(command),
    }
}

#[cfg(test)]
mod tests;
