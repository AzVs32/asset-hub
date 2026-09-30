mod runtime;

use std::path::PathBuf;
use std::process::ExitCode;

use asset_vfs::mount::{Mount, MountId};
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
    /// Query persisted mount definitions.
    Mount {
        #[command(subcommand)]
        command: MountCommand,
    },
}

#[derive(Subcommand)]
enum MountCommand {
    /// List all mounts, including disabled mounts.
    List,
    /// Show a mount by its ID.
    Info { id: MountId },
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
        Command::Mount { command } => match command {
            MountCommand::List => {
                let mounts = runtime.mount_service().list_mounts().await?;
                if mounts.is_empty() {
                    println!("No mounts configured.");
                } else {
                    print_mounts(&mounts);
                }
            }
            MountCommand::Info { id } => {
                let mount = runtime
                    .mount_service()
                    .mount_info(id)
                    .await?
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("mount not found: {id}"),
                        )
                    })?;
                print_mounts(&[mount]);
            }
        },
    }
    Ok(())
}

fn print_mounts(mounts: &[Mount]) {
    let mut table = Table::new();
    table.load_style(UTF8_FULL_CONDENSED).set_header([
        "ID",
        "VIRTUAL_PATH",
        "DRIVER",
        "DRIVER_PATH",
        "ENABLED",
    ]);
    for mount in mounts {
        table.add_row([
            mount.id().to_string(),
            mount.virtual_path().as_str().to_owned(),
            mount.driver().as_str().to_owned(),
            mount.driver_path().as_str().to_owned(),
            mount.enabled().to_string(),
        ]);
    }
    println!("{table}");
}
