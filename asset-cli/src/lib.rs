use asset_core::port::SecurityAuditRepository;
use asset_core::service::{ResourceService, UserService};
use async_trait::async_trait;
use clap::{Parser, Subcommand};
use std::path::Path;
use std::sync::Arc;

mod commands;

use commands::{config, plugin, system, user};

pub type CliResult<T = ()> = anyhow::Result<T>;

/// Core-facing services needed by maintenance CLI commands.
pub struct CliServices {
    resource: ResourceService,
    users: UserService,
    audit: Arc<dyn SecurityAuditRepository>,
}

impl CliServices {
    pub fn new(
        resource: ResourceService,
        users: UserService,
        audit: Arc<dyn SecurityAuditRepository>,
    ) -> Self {
        Self {
            resource,
            users,
            audit,
        }
    }

    pub fn resource_service(&self) -> &ResourceService {
        &self.resource
    }

    pub fn user_service(&self) -> &UserService {
        &self.users
    }

    pub fn security_audit_repository(&self) -> &Arc<dyn SecurityAuditRepository> {
        &self.audit
    }
}

/// Host capabilities required by the CLI adapter.
///
/// The concrete host may use `asset-infra`, while this crate remains dependent
/// only on core-facing services and serialized configuration output.
#[async_trait]
pub trait CliHost: Send + Sync {
    async fn maintenance_services(&self) -> CliResult<CliServices>;
    fn validate_config(&self, path: Option<&Path>) -> CliResult;
    fn normalized_config_toml(&self, path: Option<&Path>) -> CliResult<String>;
}

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

pub async fn run(cli: Cli, host: &impl CliHost) -> CliResult {
    match cli.command {
        Command::Config(command) => config::run(command, host),
        Command::System(command) => system::run(command, host).await,
        Command::User(command) => user::run(command, host).await,
        Command::Plugin(command) => plugin::run(command),
    }
}

#[cfg(test)]
mod tests;
