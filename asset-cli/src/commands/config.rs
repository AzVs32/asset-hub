use crate::CliResult;
use asset_infra::config::AssetInfraConfig;
use clap::{ArgGroup, Args};
use std::path::Path;

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("operation")
        .required(true)
        .multiple(false)
        .args(["check", "show"])
))]
pub(crate) struct ConfigCommand {
    /// Validate a configuration file without initializing the application runtime.
    #[arg(long)]
    check: bool,

    /// Print the normalized configuration as TOML.
    #[arg(long)]
    show: bool,
}

pub(crate) fn run(command: ConfigCommand, config_path: Option<&Path>) -> CliResult {
    match (command.check, command.show) {
        (true, false) => {
            load_normalized_config(config_path)?;
            println!("configuration is valid");
        }
        (false, true) => {
            print!(
                "{}",
                toml::to_string_pretty(&load_normalized_config(config_path)?)?
            );
        }
        (false, false) | (true, true) => {
            unreachable!("clap requires exactly one config operation")
        }
    }
    Ok(())
}

fn load_normalized_config(path: Option<&Path>) -> Result<AssetInfraConfig, asset_core::CoreError> {
    match path {
        Some(path) => AssetInfraConfig::from_config_file(path),
        None => AssetInfraConfig::from_default_config_file(),
    }?
    .normalized()
}
