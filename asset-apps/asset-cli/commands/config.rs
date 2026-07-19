use super::CommandResult;
use asset_infra::config::AssetInfraConfig;
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

pub(crate) fn run(command: ConfigCommand) -> CommandResult {
    match (command.check, command.show) {
        (Some(path), None) => {
            load_normalized(path)?;
            println!("configuration is valid");
        }
        (None, Some(path)) => {
            let config = load_normalized(path)?;
            print!("{}", toml::to_string_pretty(&config)?);
        }
        (None, None) | (Some(_), Some(_)) => {
            unreachable!("clap requires exactly one config operation")
        }
    }
    Ok(())
}

fn load_normalized(path: Option<PathBuf>) -> Result<AssetInfraConfig, asset_core::CoreError> {
    match path {
        Some(path) => AssetInfraConfig::from_config_file(path),
        None => AssetInfraConfig::from_default_config_file(),
    }?
    .normalized()
}
