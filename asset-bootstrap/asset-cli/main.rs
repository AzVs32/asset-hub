use asset_bootstrap::AssetRuntime;
use asset_cli::{Cli, CliHost, CliResult, CliServices};
use asset_infra::config::AssetInfraConfig;
use async_trait::async_trait;
use clap::Parser;
use std::path::Path;

struct DefaultCliHost;

#[async_trait]
impl CliHost for DefaultCliHost {
    async fn maintenance_services(&self) -> CliResult<CliServices> {
        let runtime = AssetRuntime::from_default_config_file_without_storage_sync().await?;
        Ok(CliServices::new(
            runtime.resource_service(),
            runtime.user_service(),
            runtime.security_audit_repository(),
        ))
    }

    fn validate_config(&self, path: Option<&Path>) -> CliResult {
        load_normalized_config(path).map(|_| ()).map_err(Into::into)
    }

    fn normalized_config_toml(&self, path: Option<&Path>) -> CliResult<String> {
        Ok(toml::to_string_pretty(&load_normalized_config(path)?)?)
    }
}

fn load_normalized_config(path: Option<&Path>) -> Result<AssetInfraConfig, asset_core::CoreError> {
    match path {
        Some(path) => AssetInfraConfig::from_config_file(path),
        None => AssetInfraConfig::from_default_config_file(),
    }?
    .normalized()
}

#[tokio::main]
async fn main() {
    if let Err(error) = asset_cli::run(Cli::parse(), &DefaultCliHost).await {
        eprintln!("asset: {error}");
        std::process::exit(1);
    }
}
