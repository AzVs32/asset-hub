use crate::CliResult;
use clap::{ArgGroup, Args};
use std::path::{Component, Path, PathBuf};

mod tool;

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["seal", "verify"])
))]
pub(crate) struct Command {
    /// Seal an installed plugin package by ID and generate its lock.
    #[arg(long, value_name = "PLUGIN_ID")]
    seal: Option<String>,
    /// Verify a sealed installed plugin package by ID without changing it.
    #[arg(long, value_name = "PLUGIN_ID")]
    verify: Option<String>,
}

pub(crate) fn run(command: Command, packages_root: &Path) -> CliResult {
    let (plugin_id, operation) = match (command.seal, command.verify) {
        (None, Some(plugin_id)) => (plugin_id, Operation::Verify),
        (Some(plugin_id), None) => (plugin_id, Operation::Seal),
        (None, None) => anyhow::bail!("one of --seal or --verify is required"),
        (Some(_), Some(_)) => unreachable!("clap rejects conflicting plugin operations"),
    };
    let package = package_path(packages_root, &plugin_id)?;

    match operation {
        Operation::Verify => {
            let plugin = tool::verify_package(&package)?;
            println!(
                "verified plugin `{}` ({})",
                plugin.plugin_id(),
                package.display()
            );
            Ok(())
        }
        Operation::Seal => {
            let plugin = tool::generate_lock(&package)?;
            println!(
                "sealed plugin `{}` ({})",
                plugin.plugin_id(),
                package.display()
            );
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Seal,
    Verify,
}

fn package_path(packages_root: &Path, plugin_id: &str) -> CliResult<PathBuf> {
    let mut components = Path::new(plugin_id).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        anyhow::bail!("plugin id must be a single package directory name: `{plugin_id}`");
    }
    Ok(packages_root.join(plugin_id))
}
