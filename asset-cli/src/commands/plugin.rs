use crate::CliResult;
use clap::Args;
use std::path::PathBuf;

mod tool;

#[derive(Debug, Args)]
pub(crate) struct PluginCommand {
    /// Verify a plugin package that already has a generated lock.
    #[arg(long, value_name = "MANIFEST", required = true)]
    verify: PathBuf,
}

pub(crate) fn run(command: PluginCommand) -> CliResult {
    let path = command.verify;
    let plugin = tool::verify_manifest(&path)?;
    println!(
        "verified plugin `{}` ({})",
        plugin.plugin_id(),
        path.display()
    );
    Ok(())
}
