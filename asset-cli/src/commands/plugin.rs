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
    /// Calculate artifact integrity and atomically write manifest.lock.json.
    #[arg(long, value_name = "MANIFEST")]
    seal: Option<PathBuf>,

    /// Verify a sealed plugin package without modifying it.
    #[arg(long, value_name = "MANIFEST")]
    verify: Option<PathBuf>,
}

pub(crate) fn run(command: PluginCommand) -> CliResult {
    let (operation, path, plugin) = match (command.seal, command.verify) {
        (Some(path), None) => {
            let plugin = tool::seal_manifest(&path)?;
            ("sealed", path, plugin)
        }
        (None, Some(path)) => {
            let plugin = tool::verify_manifest(&path)?;
            ("verified", path, plugin)
        }
        (None, None) | (Some(_), Some(_)) => {
            unreachable!("clap requires exactly one plugin operation")
        }
    };
    println!(
        "{} plugin `{}` ({})",
        operation,
        plugin.plugin_id(),
        path.display()
    );
    Ok(())
}
