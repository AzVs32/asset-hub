mod runtime;

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;

use asset_vfs::driver::{DriverKind, DriverPath};
use asset_vfs::error::VfsError;
use asset_vfs::mount::{Mount, MountId, MountInfo, MountSelector, ResolvedMount};
use asset_vfs::namespace::VirtualPath;
use asset_vfs::{DriverService, MountService};
use clap::{Parser, Subcommand};
use comfy_table::{Table, presets::UTF8_FULL_CONDENSED};
use runtime::{Runtime, RuntimeError, RuntimeOptions};

#[derive(Parser)]
#[command(version, about = "CLI for exercising Asset Hub services")]
struct Cli {
    /// Configuration file (defaults to config.toml in the working directory).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Query registered driver kinds and declarations.
    Driver {
        #[command(subcommand)]
        command: DriverCommand,
    },
    /// Manage and query persisted mount definitions.
    Mount {
        #[command(subcommand)]
        command: MountCommand,
    },
}

#[derive(Subcommand)]
enum DriverCommand {
    /// List all registered driver kinds.
    List,
}

#[derive(Subcommand)]
enum MountCommand {
    /// List all mounts, including disabled mounts.
    List {
        /// Include IDs even when virtual paths distinguish every row.
        #[arg(long)]
        show_ids: bool,
    },
    /// Show the mount at an exact virtual path, or a definition by ID.
    Info {
        #[arg(value_name = "VPATH_OR_MOUNT_ID")]
        target: MountSelector,
        #[arg(long)]
        show_ids: bool,
    },
    /// Add a mount; the backend root must already exist when enabled.
    Add {
        #[arg(value_parser = parse_virtual_path)]
        vpath: VirtualPath,
        #[arg(value_parser = parse_driver_kind)]
        driver: DriverKind,
        /// Driver-specific root; local requires an absolute filesystem path.
        driver_path: String,
        /// Save a disabled definition without binding its backend.
        #[arg(long)]
        disabled: bool,
    },
    /// Re-enable a saved mount, retaining its ID.
    Enable {
        #[arg(value_name = "VPATH_OR_MOUNT_ID")]
        target: MountSelector,
    },
    /// Disable a mount while keeping its saved definition.
    Unmount {
        #[arg(value_name = "VPATH_OR_MOUNT_ID")]
        target: MountSelector,
    },
    /// Find the deepest enabled mount covering a virtual path.
    Resolve {
        #[arg(value_parser = parse_virtual_path)]
        vpath: VirtualPath,
    },
}

fn parse_virtual_path(value: &str) -> Result<VirtualPath, VfsError> {
    VirtualPath::try_from(value)
}

fn parse_driver_kind(value: &str) -> Result<DriverKind, VfsError> {
    DriverKind::try_from(value)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), RuntimeError> {
    let options = cli
        .config
        .map(RuntimeOptions::with_config_file)
        .unwrap_or_default();
    let runtime = Runtime::initialize(options).await?;
    match cli.command {
        Command::Driver { command } => match command {
            DriverCommand::List => print_drivers(runtime.driver_service())?,
        },
        Command::Mount { command } => match command {
            MountCommand::List { show_ids } => {
                let mounts = runtime.mount_service().list_mounts().await?;
                if mounts.is_empty() {
                    println!("No mounts configured.");
                } else {
                    print_mounts(&mounts, show_ids);
                }
            }
            MountCommand::Info { target, show_ids } => {
                let mount = runtime
                    .mount_service()
                    .mount_info(target.clone())
                    .await?
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("mount not found: {target}"),
                        )
                    })?;
                print_mounts(&[mount], show_ids || matches!(target, MountSelector::Id(_)));
            }
            MountCommand::Add {
                vpath,
                driver,
                driver_path,
                disabled,
            } => {
                let mount = Mount::new(
                    MountId::new(),
                    vpath,
                    driver,
                    DriverPath::new(driver_path),
                    !disabled,
                );
                let info = runtime.mount_service().mount(mount).await?;
                print_mount_change(runtime.mount_service(), info, false).await?;
            }
            MountCommand::Enable { target } => {
                let info = runtime
                    .mount_service()
                    .mount_info(target.clone())
                    .await?
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("mount not found: {target}"),
                        )
                    })?;
                let mut mount = info.mount().clone();
                mount.enable();
                let info = runtime.mount_service().mount(mount).await?;
                print_mount_change(
                    runtime.mount_service(),
                    info,
                    matches!(target, MountSelector::Id(_)),
                )
                .await?;
            }
            MountCommand::Unmount { target } => {
                let info = runtime
                    .mount_service()
                    .unmount(target.clone())
                    .await?
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("mount not found: {target}"),
                        )
                    })?;
                print_mount_change(
                    runtime.mount_service(),
                    info,
                    matches!(target, MountSelector::Id(_)),
                )
                .await?;
            }
            MountCommand::Resolve { vpath } => {
                let resolved = runtime
                    .mount_service()
                    .resolve_mount(&vpath)
                    .await?
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("no enabled mount covers: {vpath}"),
                        )
                    })?;
                print_resolved(&resolved);
            }
        },
    }
    Ok(())
}

fn print_drivers(service: &DriverService) -> Result<(), RuntimeError> {
    if service.is_empty() {
        println!("No drivers registered.");
        return Ok(());
    }
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL_CONDENSED)
        .set_header(["DRIVER", "ALLOWS_SUBMOUNTS"]);
    for kind in service.list() {
        let info = service.require(&kind)?;
        table.add_row([
            info.kind().as_str().to_owned(),
            info.allows_submounts().to_string(),
        ]);
    }
    println!("{table}");
    Ok(())
}

async fn print_mount_change(
    service: &MountService,
    info: MountInfo,
    show_ids: bool,
) -> Result<(), VfsError> {
    let mount = info.mount();
    let show_ids = show_ids
        || (!mount.enabled()
            && service.list_mounts().await?.iter().any(|other| {
                other.mount().id() != mount.id()
                    && other.mount().virtual_path() == mount.virtual_path()
            }));
    print_mounts(&[info], show_ids);
    Ok(())
}

fn print_mounts(mounts: &[MountInfo], show_ids: bool) {
    let mut paths = HashSet::new();
    let show_ids = show_ids
        || mounts
            .iter()
            .any(|info| !paths.insert(info.mount().virtual_path()));
    let mut table = Table::new();
    let mut headers = vec![
        "VIRTUAL_PATH",
        "DRIVER",
        "DRIVER_PATH",
        "ENABLED",
        "ALLOWS_SUBMOUNTS",
    ];
    if show_ids {
        headers.insert(1, "ID");
    }
    table.load_style(UTF8_FULL_CONDENSED).set_header(headers);
    for info in mounts {
        let mount = info.mount();
        let mut row = vec![
            mount.virtual_path().as_str().to_owned(),
            mount.driver().as_str().to_owned(),
            mount.driver_path().as_str().to_owned(),
            mount.enabled().to_string(),
            info.allows_submounts()
                .map(|allowed| allowed.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
        ];
        if show_ids {
            row.insert(1, mount.id().to_string());
        }
        table.add_row(row);
    }
    println!("{table}");
}

fn print_resolved(resolved: &ResolvedMount) {
    let mount = resolved.mount();
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL_CONDENSED)
        .set_header(["VIRTUAL_PATH", "RELATIVE_PATH", "DRIVER", "DRIVER_PATH"])
        .add_row([
            mount.virtual_path().as_str(),
            resolved.relative_path().as_str(),
            mount.driver().as_str(),
            mount.driver_path().as_str(),
        ]);
    println!("{table}");
}
