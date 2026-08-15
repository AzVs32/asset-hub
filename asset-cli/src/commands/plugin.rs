use crate::CliResult;
use clap::{ArgGroup, Args};
use comfy_table::{Table, presets::UTF8_FULL};
use std::path::{Path, PathBuf};

mod tool;

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["list", "install", "uninstall"])
))]
pub(crate) struct Command {
    /// List all installed plugins.
    #[arg(long)]
    list: bool,
    /// Install a plugin from a local package directory.
    #[arg(long, value_name = "PATH")]
    install: Option<PathBuf>,
    /// Uninstall an installed plugin by ID.
    #[arg(long, value_name = "PLUGIN_ID")]
    uninstall: Option<String>,
}

pub(crate) fn run(command: Command, packages_root: &Path) -> CliResult {
    if command.list {
        list(packages_root)
    } else if let Some(source) = command.install {
        install(&source, packages_root)
    } else if let Some(plugin_id) = command.uninstall {
        uninstall(&plugin_id, packages_root)
    } else {
        unreachable!("clap requires exactly one plugin operation")
    }
}

fn list(packages_root: &Path) -> CliResult {
    let plugins = tool::list_packages(packages_root)?;
    if plugins.is_empty() {
        println!("no plugins installed");
        return Ok(());
    }
    let mut table = Table::new();
    table.load_style(UTF8_FULL);
    table.set_header(["ID", "NAME", "VERSION", "PUBLISHER"]);
    for plugin in plugins {
        table.add_row([plugin.id, plugin.name, plugin.version, plugin.publisher]);
    }
    println!("{table}");
    Ok(())
}

fn install(source: &Path, packages_root: &Path) -> CliResult {
    let source_text = source.to_string_lossy();
    if source_text.contains("://") || source_text.starts_with("git@") {
        anyhow::bail!(
            "remote plugin sources are not supported yet; --install expects a local directory path"
        );
    }
    let installed = tool::install_package(source, packages_root)?;
    let replacement = if installed.replaced_existing() {
        "; replaced existing installation"
    } else {
        ""
    };
    println!(
        "installed plugin `{}` from `{}` to `{}`{replacement}",
        installed.manifest().plugin_id(),
        source.display(),
        installed.package_root().display()
    );
    Ok(())
}

fn uninstall(plugin_id: &str, packages_root: &Path) -> CliResult {
    let removed = tool::uninstall_package(packages_root, plugin_id)?;
    println!(
        "uninstalled plugin `{plugin_id}` from `{}`",
        removed.display()
    );
    Ok(())
}
