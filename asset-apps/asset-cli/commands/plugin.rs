use super::CliResult;
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct PluginCommand {}

pub(crate) fn run(_command: PluginCommand) -> CliResult {
    Ok(())
}
