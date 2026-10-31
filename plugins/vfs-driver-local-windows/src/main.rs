//! Native WindowsLocalDriver plugin executable. Filesystem operations run on this host.
#[cfg(target_os = "windows")]
use std::{env, sync::Arc};

#[cfg(target_os = "windows")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    let listen = args.next().unwrap_or_else(|| "127.0.0.1:0".into());
    let database = args.next().unwrap_or_else(|| "local-plugin.sqlite".into());
    if args.next().is_some() {
        return Err("usage: vfs-driver-local-windows [listen-address] [database-file]".into());
    }
    let token = env::var("VFS_PLUGIN_TOKEN").map_err(|_| "VFS_PLUGIN_TOKEN is required")?;
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(database)
        .create_if_missing(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;
    let driver = Arc::new(vfs_driver_local_windows::WindowsLocalDriver::from_pool(pool).await?);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    let tls = match (
        env::var_os("VFS_PLUGIN_TLS_CERT"),
        env::var_os("VFS_PLUGIN_TLS_KEY"),
    ) {
        (None, None) => None,
        (Some(cert), Some(key)) => Some(vfs_plugin_grpc_server::ServerTlsConfig::new().identity(
            vfs_plugin_grpc_server::Identity::from_pem(std::fs::read(cert)?, std::fs::read(key)?),
        )),
        _ => {
            return Err(
                "VFS_PLUGIN_TLS_CERT and VFS_PLUGIN_TLS_KEY must be configured together".into(),
            );
        }
    };
    println!("{}", listener.local_addr()?);
    vfs_plugin_grpc_server::serve_configured(listener, driver, &token, tls, std::future::pending()).await
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("vfs-driver-local-windows supports Windows only");
    std::process::exit(2);
}
