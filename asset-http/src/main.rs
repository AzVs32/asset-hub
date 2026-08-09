use asset_http::{HttpSessionRuntime, HttpSettings, build_router, with_authentication};
use asset_infra::config::AssetInfraConfig;
use asset_runtime::AssetRuntime;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let settings = HttpSettings::from_cli();
    let config = match settings.config_path() {
        Some(path) => AssetInfraConfig::from_config_file(path)?,
        None => AssetInfraConfig::from_default_config_file()?,
    }
    .normalized()?;
    // info!(config = ?config, "asset-http config");
    let mut runtime = AssetRuntime::new(config).await?;
    runtime.start_storage_sync().await?;
    let session_runtime = HttpSessionRuntime::new().await?;
    let listener = tokio::net::TcpListener::bind(settings.addr()).await?;

    info!(addr = %settings.addr(), "asset-http listening");
    let authorization = runtime.authorization_service();
    let app = build_router(
        runtime.resource_service(),
        settings.router_options().clone(),
        runtime.plugin_web_assets(),
        authorization.clone(),
        runtime.upload_finalization_dispatcher(),
    );
    let app = with_authentication(
        app,
        runtime.user_service(),
        session_runtime.store(),
        session_runtime.health(),
        settings.session_options(),
    )?;

    axum::serve(listener, app).await?;

    Ok(())
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(
            "asset_http=info,asset_runtime=info,asset_infra=info,asset_core=info,tower_http=info",
        )
    });

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}
