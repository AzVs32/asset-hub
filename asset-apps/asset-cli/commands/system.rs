use super::CliResult;
use asset_apps::AssetRuntime;
use clap::{ArgGroup, Args};
use std::io::Write;
use std::time::Duration;

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["scan_resource"])
))]
pub(crate) struct SystemCommand {
    /// Fully verify every stored resource and recalculate its SHA-256 checksum.
    #[arg(long)]
    scan_resource: bool,
}

pub(crate) async fn run(command: SystemCommand) -> CliResult {
    if command.scan_resource {
        let runtime = AssetRuntime::from_default_config_file_without_storage_sync().await?;
        println!("verifying all stored resources with SHA-256...");
        std::io::stdout().flush()?;
        let report = runtime.resource_service().scan_resources().await?;
        println!(
            "verified {} resources ({} hashed, {} directories) in {}",
            report.files,
            report.hashed_files,
            report.directories,
            format_duration(report.elapsed),
        );
        println!(
            "SHA-256 calculation time: {}",
            format_duration(report.hash_elapsed)
        );
    } else {
        unreachable!("clap requires exactly one system operation");
    }
    Ok(())
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}s", duration.as_secs_f64())
}
