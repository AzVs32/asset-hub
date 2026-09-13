use crate::{CliResult, load_config};
use clap::{ArgGroup, Args};
use std::path::Path;

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["check", "show"])
))]
pub(crate) struct Command {
    /// Validate a configuration file without initializing the application runtime.
    #[arg(long)]
    check: bool,

    /// Print the normalized configuration as TOML.
    #[arg(long)]
    show: bool,
}

pub(crate) fn run(command: Command, config_path: Option<&Path>) -> CliResult {
    match (command.check, command.show) {
        (true, false) => {
            load_config(config_path)?;
            println!("[asset] configuration is valid; unregistered sections were not validated");
        }
        (false, true) => {
            print!("{}", load_config(config_path)?.to_toml_string()?);
        }
        (false, false) | (true, true) => {
            unreachable!("clap requires exactly one config operation")
        }
    }
    Ok(())
}
