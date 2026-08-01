use crate::CliResult;
use clap::Args;
use std::path::PathBuf;

mod tool;

#[derive(Debug, Args)]
pub(crate) struct PluginCommand {
    /// Verify a sealed plugin package without changing it.
    #[arg(
        long,
        value_name = "MANIFEST",
        required_unless_present = "generate_lock",
        conflicts_with = "generate_lock"
    )]
    verify: Option<PathBuf>,
    /// Generate the lock for an unsealed plugin package.
    #[arg(
        long,
        value_name = "MANIFEST",
        required_unless_present = "verify",
        conflicts_with = "verify"
    )]
    generate_lock: Option<PathBuf>,
}

pub(crate) fn run(command: PluginCommand) -> CliResult {
    match (command.verify, command.generate_lock) {
        (Some(path), None) => {
            let plugin = tool::verify_manifest(&path)?;
            println!(
                "verified plugin `{}` ({})",
                plugin.plugin_id(),
                path.display()
            );
            Ok(())
        }
        (None, Some(path)) => {
            let plugin = tool::generate_lock(&path)?;
            println!(
                "generated lock for plugin `{}` ({})",
                plugin.plugin_id(),
                path.display()
            );
            Ok(())
        }
        (None, None) => anyhow::bail!("one of --verify or --generate-lock is required"),
        (Some(_), Some(_)) => unreachable!("clap rejects conflicting plugin operations"),
    }
}
