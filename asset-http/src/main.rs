use asset_http::{
    ArchiveRuntime, DirectoryHttpServices, HttpComposition, HttpHealthServices, HttpServices,
    HttpSettings, ResourceHttpServices, build_router,
};
use asset_infra::config::AssetInfraConfig;
use asset_runtime::AssetRuntime;
use std::future::IntoFuture;
use std::time::Duration;
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
    let mut runtime = AssetRuntime::new(config).await?;
    let mut archives = ArchiveRuntime::new(settings.archive_options().clone());
    let result = serve(&mut runtime, &archives, settings).await;
    // Also clean up on listener/startup errors. Keep the task owner alive until all workers exit.
    archives.shutdown().await;
    result
}

async fn serve(
    runtime: &mut AssetRuntime,
    archives: &ArchiveRuntime,
    settings: HttpSettings,
) -> Result<(), Box<dyn std::error::Error>> {
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
                    storage_health: runtime.storage_health_service(),
                },
            },
            upload_finalizations: runtime.upload_finalization_dispatcher(),
            archives: archives.downloads(),
        },
        settings.router_options().clone(),
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        signal = shutdown_signal() => {
            signal?;
            archives.begin_shutdown();
            let _ = stop.send(());
            info!("stopping HTTP admission and cancelling archive work");
            match tokio::time::timeout(Duration::from_secs(30), &mut server).await {
                Ok(result) => result?,
                Err(_) => tracing::warn!("HTTP drain deadline expired; remaining requests stop at process exit"),
            }
        }
    }

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

// Signal and connection-drain policy belong to the HTTP executable.
async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}
