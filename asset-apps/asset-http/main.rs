mod auth;
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
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let settings = HttpSettings::from_cli();
    let runtime = AssetRuntime::from_optional_config_file(settings.config_path()).await?;
    let listener = tokio::net::TcpListener::bind(settings.addr()).await?;

    info!(addr = %settings.addr(), "asset-http listening");
    info!(
        config_file = %settings
            .config_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| settings.default_config_file().to_string()),
        "asset-http config file"
    );
    info!(config = ?runtime.config(), "asset-http config");

    let authorization = runtime.authorization_service();
    let app = router::build_with_options_and_plugin_web_roots(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        settings.router_options().clone(),
        runtime.plugin_web_roots()?,
        authorization.clone(),
    );
    let bootstrap_username = std::env::var("ASSET_HUB_BOOTSTRAP_ADMIN_USERNAME").ok();
    let bootstrap_password = std::env::var("ASSET_HUB_BOOTSTRAP_ADMIN_PASSWORD").ok();
    let bootstrap_admin = bootstrap_username
        .as_deref()
        .zip(bootstrap_password.as_deref());
    let app = router::with_authentication(
        app,
        runtime.user_service(),
        authorization,
        &runtime.config().database.sqlite_path,
        bootstrap_admin,
        settings.session_options(),
    )
    .await?;

    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("asset_http=info,tower_http=info"));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}
