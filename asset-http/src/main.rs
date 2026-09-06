use asset_http::{
    DirectoryHttpServices, HttpComposition, HttpHealthServices, HttpServices, HttpSettings,
    ResourceHttpServices, build_router,
};
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
    let listener = tokio::net::TcpListener::bind(settings.addr()).await?;

    info!(addr = %settings.addr(), "asset-http listening");
    let app = build_router(
        HttpComposition {
            services: HttpServices {
                resources: ResourceHttpServices {
                    resources: runtime.resource_service(),
                    content: runtime.content_service(),
                    uploads: runtime.upload_service(),
                },
                directories: DirectoryHttpServices {
                    directories: runtime.directory_service(),
                },
                workflows: runtime.asset_workflow_service(),
                health: HttpHealthServices {
                    storage_maintenance: runtime.storage_maintenance_service(),
                },
            },
            upload_finalizations: runtime.upload_finalization_dispatcher(),
        },
        settings.router_options().clone(),
    );
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
