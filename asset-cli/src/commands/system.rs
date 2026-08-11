use crate::CliResult;
use asset_core::service::{ResourceScanProgress, ResourceService, StorageReconciliationReport};
use clap::{ArgGroup, Args};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::Write;
use std::time::Duration;

/// Arguments for trusted system maintenance operations.
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

pub(crate) async fn run(command: SystemCommand, service: ResourceService) -> CliResult {
    if command.scan_resource {
        scan_resources(service).await
    } else {
        unreachable!("clap requires exactly one system operation");
    }
}

/// Scans every stored resource while rendering file-level progress.
async fn scan_resources(service: ResourceService) -> CliResult {
    println!("verifying all stored resources with SHA-256...");
    std::io::stdout().flush()?;

    let progress = scan_progress_bar()?;
    // indicatif 的 clone 共享同一状态，可安全移入进度回调并由当前函数负责收尾。
    let rendered_progress = progress.clone();
    let result = service
        .scan_resources_with_progress(move |state| {
            render_scan_progress(&rendered_progress, state);
        })
        .await;

    match result {
        Ok(report) => {
            // 清除动态输出，避免干扰后续的最终扫描摘要。
            progress.finish_and_clear();
            print_scan_report(&report);
            Ok(())
        }
        Err(error) => {
            // 失败时保留一条稳定消息，再将原始错误交给顶层 CLI 输出。
            progress.abandon_with_message("resource scan failed");
            Err(error.into())
        }
    }
}

/// Updates the terminal indicator from a resource scan progress event.
fn render_scan_progress(progress: &ProgressBar, state: ResourceScanProgress) {
    match state {
        ResourceScanProgress::Discovering { files } => {
            // 枚举完成前没有总数，使用转圈提示已发现的文件数量。
            progress.set_message(format!("discovering resources ({files} files)"));
        }
        ResourceScanProgress::Verifying {
            completed_files,
            total_files,
            current_file,
        } => {
            // 获取总数后切换为确定进度条，并持续展示当前文件路径。
            progress.set_style(verification_style());
            progress.set_length(total_files);
            progress.set_position(completed_files);
            progress.set_message(match current_file {
                Some(key) => format!("checking {key}"),
                None => "waiting for next file".to_owned(),
            });
        }
    }
}

/// Prints the final resource scan summary.
fn print_scan_report(report: &StorageReconciliationReport) {
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
}

/// Builds the indeterminate progress indicator used while resources are discovered.
fn scan_progress_bar() -> CliResult<ProgressBar> {
    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::with_template("{spinner:.cyan} [{elapsed_precise}] {msg}")?
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    progress.enable_steady_tick(Duration::from_millis(100));
    Ok(progress)
}

/// Builds the determinate style used once the total file count is known.
fn verification_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
    )
    .expect("static progress template should be valid")
    .progress_chars("=>-")
}

/// Formats a duration for the command summary.
fn format_duration(duration: Duration) -> String {
    format!("{:.3}s", duration.as_secs_f64())
}
