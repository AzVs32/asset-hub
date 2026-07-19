use super::CommandResult;
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct UserCommand {}

pub(crate) fn run(_command: UserCommand) -> CommandResult {
    Ok(())
}
