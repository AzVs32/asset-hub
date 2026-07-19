use super::CommandResult;
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct SystemCommand {}

pub(crate) fn run(_command: SystemCommand) -> CommandResult {
    Ok(())
}
