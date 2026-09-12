use asset_config::ConfigRegistry;
use asset_http::{
    ArchiveRuntime, DirectoryHttpServices, HttpArgs, HttpComposition, HttpConfig,
    HttpHealthServices, HttpRuntimeOptions, HttpServices, ResourceHttpServices, build_router,
};
use asset_runtime::{AssetConfig, AssetRuntime};
use std::future::IntoFuture;
use std::time::Duration;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let args = HttpArgs::from_cli();
    let config = ConfigRegistry::new()
        .with::<AssetConfig>()?
        .with::<HttpConfig>()?
        .load(args.config_path())?;
    let asset_config = config.section::<AssetConfig>()?.clone();
    let HttpRuntimeOptions {
        addr,
        archive,
        router,
    } = config
        .section::<HttpConfig>()?
        .clone()
        .into_runtime_options()?;

    let mut runtime = AssetRuntime::new(asset_config).await?;
    let mut archives = ArchiveRuntime::new(archive);
    let result = serve(&mut runtime, &archives, addr, router).await;
    // Also clean up on listener/startup errors. Keep the task owner alive until all workers exit.
    archives.shutdown().await;
    result
}

async fn serve(
    runtime: &mut AssetRuntime,
    archives: &ArchiveRuntime,
    addr: std::net::SocketAddr,
    router_options: asset_http::RouterOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    runtime.start_storage_sync().await?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!(addr = %addr, "asset-http listening");
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
        router_options,
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
