use crate::{CliHost, CliResult};
use clap::{ArgGroup, Args};
use std::path::PathBuf;

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["check", "show"])
))]
pub(crate) struct ConfigCommand {
    /// Validate a configuration file without initializing the application runtime.
    #[arg(long, value_name = "PATH", num_args = 0..=1)]
    check: Option<Option<PathBuf>>,

    /// Print the normalized configuration as TOML.
    #[arg(long, value_name = "PATH", num_args = 0..=1)]
    show: Option<Option<PathBuf>>,
}

pub(crate) fn run(command: ConfigCommand, host: &impl CliHost) -> CliResult {
    match (command.check, command.show) {
        (Some(path), None) => {
            host.validate_config(path.as_deref())?;
            println!("configuration is valid");
        }
        (None, Some(path)) => {
            print!("{}", host.normalized_config_toml(path.as_deref())?);
        }
        (None, None) | (Some(_), Some(_)) => {
            unreachable!("clap requires exactly one config operation")
        }
    }
    Ok(())
}
