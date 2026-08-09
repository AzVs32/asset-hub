use crate::CliResult;
use clap::{ArgGroup, Args};
use std::path::PathBuf;

mod tool;

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["seal", "verify"])
))]
pub(crate) struct PluginCommand {
    /// Validate and seal a plugin package by generating its lock.
    #[arg(long, value_name = "PACKAGE")]
    seal: Option<PathBuf>,
    /// Verify a sealed plugin package without changing it.
    #[arg(long, value_name = "PACKAGE")]
    verify: Option<PathBuf>,
}

pub(crate) fn run(command: PluginCommand) -> CliResult {
    match (command.seal, command.verify) {
        (None, Some(package)) => {
            let plugin = tool::verify_package(&package)?;
            println!(
                "verified plugin `{}` ({})",
                plugin.plugin_id(),
                package.display()
            );
            Ok(())
        }
        (Some(package), None) => {
            let plugin = tool::generate_lock(&package)?;
            println!(
                "sealed plugin `{}` ({})",
                plugin.plugin_id(),
                package.display()
            );
            Ok(())
        }
        (None, None) => anyhow::bail!("one of --seal or --verify is required"),
        (Some(_), Some(_)) => unreachable!("clap rejects conflicting plugin operations"),
    }
}
