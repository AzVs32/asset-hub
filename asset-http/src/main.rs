use asset_http::{HttpSettings, build_router, with_authentication};
use asset_infra::config::AssetInfraConfig;
use asset_runtime::AssetRuntime;
use tower_sessions::session_store::ExpiredDeletion;
use tower_sessions_sqlx_store::SqliteStore;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let settings = HttpSettings::from_cli();
    let config = match settings.config_path() {
        Some(path) => AssetInfraConfig::from_config_file(path)?,
        None => AssetInfraConfig::from_default_config_file()?,
    };
    let mut runtime = AssetRuntime::new(config).await?;
    runtime.start_storage_sync().await?;
    let listener = tokio::net::TcpListener::bind(settings.addr()).await?;

    info!(addr = %settings.addr(), "asset-http listening");
    info!(config = ?runtime.config(), "asset-http config");

    let authorization = runtime.authorization_service();
    let app = build_router(
        runtime.resource_service(),
        runtime.resource_kind_registry(),
        settings.router_options().clone(),
        runtime.plugin_web_assets(),
        authorization.clone(),
    );
    let bootstrap_username = std::env::var("ASSET_HUB_BOOTSTRAP_ADMIN_USERNAME").ok();
    let bootstrap_password = std::env::var("ASSET_HUB_BOOTSTRAP_ADMIN_PASSWORD").ok();
    let bootstrap_admin = bootstrap_username
        .as_deref()
        .zip(bootstrap_password.as_deref());
    let session_store = SqliteStore::new(runtime.database_pool());
    session_store.migrate().await?;
    tokio::spawn(
        session_store
            .clone()
            .continuously_delete_expired(std::time::Duration::from_secs(60 * 60)),
    );
    let app = with_authentication(
        app,
        runtime.user_service(),
        runtime.security_audit_repository(),
        session_store,
        bootstrap_admin,
        settings.session_options(),
    )
    .await?;

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
