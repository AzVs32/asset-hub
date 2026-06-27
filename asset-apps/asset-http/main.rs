mod dto;
mod error;
mod handlers;
mod openapi;
mod router;
mod settings;
mod state;

use asset_apps::AssetRuntime;
use settings::HttpSettings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = HttpSettings::from_env()?;
    let runtime = AssetRuntime::from_optional_config_file(settings.config_path()).await?;
    let listener = tokio::net::TcpListener::bind(settings.addr()).await?;

    println!("asset-http listening on http://{}", settings.addr());
    println!("asset-http config: {:?}", runtime.config());

    axum::serve(listener, router::build(runtime.resource_service())).await?;

    Ok(())
}
