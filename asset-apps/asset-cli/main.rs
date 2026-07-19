use clap::{Parser, Subcommand};

mod commands;

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
    Config(commands::config::ConfigCommand),
    /// Inspect and maintain the local Asset Hub system.
    System(commands::system::SystemCommand),
    /// Manage Asset Hub users.
    User(commands::user::UserCommand),
    /// Build and manage Asset Hub plugins.
    Plugin(commands::plugin::PluginCommand),
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("asset: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> commands::CommandResult {
    match cli.command {
        Command::Config(command) => commands::config::run(command),
        Command::System(command) => commands::system::run(command),
        Command::User(command) => commands::user::run(command),
        Command::Plugin(command) => commands::plugin::run(command),
    }
}

#[cfg(test)]
mod cli_tests;
