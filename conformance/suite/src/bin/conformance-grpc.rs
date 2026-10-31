//! Process boundary for the gRPC fixture; no host-side dispatch shortcuts.
use std::sync::Arc;
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let token = std::env::var("CONFORMANCE_TOKEN")?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("{}", listener.local_addr()?);
    vfs_plugin_grpc_server::serve(
        listener,
        Arc::new(conformance_native::ConformanceDriver::new()),
        &token,
        std::future::pending(),
    )
    .await
}
