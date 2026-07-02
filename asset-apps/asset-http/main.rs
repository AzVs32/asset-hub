mod dto;
mod error;
mod handlers;
mod openapi;
mod router;
mod settings;
mod state;

#[cfg(test)]
mod tests;

use asset_apps::AssetRuntime;
use settings::HttpSettings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = HttpSettings::from_env()?;
    let runtime = AssetRuntime::from_optional_config_file(settings.config_path()).await?;
    let listener = tokio::net::TcpListener::bind(settings.addr()).await?;

    println!("asset-http listening on http://{}", settings.addr());
    println!(
        "asset-http config file: {}",
        settings
            .config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| settings.default_config_file().to_string())
    );
    println!("asset-http config: {:?}", runtime.config());

    axum::serve(
        listener,
        router::build_with_options(
            runtime.resource_service(),
            runtime.resource_kind_registry(),
            settings.router_options().clone(),
        ),
    )
    .await?;

    Ok(())
}
