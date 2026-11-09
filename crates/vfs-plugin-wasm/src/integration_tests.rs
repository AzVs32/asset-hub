//! Exercise the production transport and authorization hooks with real components.
use super::*;
use ah_vfs::{
    application::{
        dto::InstanceConfig,
        port::{Driver, OpenContext},
    },
    domain::InstanceId,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vfs_plugin_host::PluginDriver;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn component() -> Option<Vec<u8>> {
    let path = std::env::var_os("VFS_WASI_TEST_COMPONENT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/wasi-probe/guest/wasm32-wasip2/debug/wasi_probe.wasm")
        });
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            eprintln!("SKIP: build the real component with scripts/build-wasi-probe.sh");
            None
        }
    }
}
async fn driver(bytes: Vec<u8>, grants: HostGrants) -> Result<Arc<PluginDriver>> {
    let driver =
        Arc::new(PluginDriver::connect(Arc::new(WasmTransport::load(bytes, grants).await?)).await?);
    driver.initialize(Default::default()).await?;
    Ok(driver)
}
fn config(command: &str, fields: &[(&str, serde_json::Value)]) -> InstanceConfig {
    let mut config = InstanceConfig::from([("command".into(), command.into())]).into_object();
    config.extend(
        fields
            .iter()
            .map(|(name, value)| ((*name).into(), value.clone())),
    );
    config.into()
}
async fn run(
    driver: &PluginDriver,
    command: &str,
    fields: &[(&str, serde_json::Value)],
) -> std::result::Result<String, DriverError> {
    let prepared = driver
        .provisioner()
        .prepare(config(command, fields))
        .await?;
    prepared
        .details()
        .get("result")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or_else(c::protocol_violation)
}
fn http_grant(origin: String) -> HostGrants {
    HostGrants {
        http: HttpGrants {
            origins: vec![origin],
            ..Default::default()
        },
        ..Default::default()
    }
}
async fn read_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                return Ok(bytes);
            }
        }
        let mut chunk = [0; 1024];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::other("request closed"));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > 32768 {
            return Err(std::io::Error::other("test request too large"));
        }
    }
}
async fn server(
    response: Vec<u8>,
) -> Result<(String, tokio::task::JoinHandle<std::io::Result<Vec<u8>>>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    Ok((
        origin,
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let request = read_request(&mut stream).await?;
            stream.write_all(&response).await?;
            stream.shutdown().await?;
            Ok(request)
        }),
    ))
}
fn response(body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend(body);
    response
}
fn denied(result: std::result::Result<String, DriverError>) -> Result {
    let error = result.err().ok_or("ungranted operation succeeded")?;
    assert_eq!(
        error.lifecycle_error().ok_or("storage category")?.failure(),
        LifecycleFailure::AccessDenied
    );
    Ok(())
}
#[tokio::test]
async fn default_grants_deny_environment_files_and_network_and_keep_std_clock_random() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let driver = driver(bytes, HostGrants::default()).await?;
    assert_eq!(
        run(&driver, "env", &[("name", "PATH".into())]).await?,
        "absent"
    );
    assert_eq!(run(&driver, "args", &[]).await?, "0");
    denied(run(&driver, "read-file", &[("path", "/etc/passwd".into())]).await)?;
    denied(run(&driver, "dns", &[("host", "ungranted.invalid".into())]).await)?;
    for address in [
        "127.0.0.1:80",
        "10.0.0.1:80",
        "169.254.169.254:80",
        "100.100.100.200:80",
        "168.63.129.16:80",
        "[::1]:80",
        "[fc00::1]:80",
        "[fe80::1]:80",
    ] {
        denied(run(&driver, "tcp", &[("address", address.into())]).await)?;
    }
    denied(run(&driver, "http", &[("url", "http://127.0.0.1:9/".into())]).await)?;
    assert!(run(&driver, "clock-random", &[]).await?.contains(':'));
    Ok(())
}
#[tokio::test]
async fn preopens_enforce_read_only_read_write_and_symlink_boundaries() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    std::fs::write(root.path().join("file"), "retained")?;
    std::fs::write(outside.path().join("secret"), "private")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("escape"))?;
    let mut grants = HostGrants {
        fs: vec![DirectoryGrant {
            host: root.path().into(),
            guest: "/data".into(),
            write: false,
        }],
        env: vec!["PATH".into()],
        ..Default::default()
    };
    let read_only = driver(bytes.clone(), grants.clone()).await?;
    assert_eq!(
        run(&read_only, "env", &[("name", "PATH".into())]).await?,
        std::env::var("PATH")?
    );
    assert_eq!(
        run(&read_only, "read-file", &[("path", "/data/file".into())]).await?,
        "retained"
    );
    denied(
        run(
            &read_only,
            "write-file",
            &[("path", "/data/file".into()), ("data", "changed".into())],
        )
        .await,
    )?;
    #[cfg(unix)]
    denied(run(&read_only, "read-file", &[("path", "/data/escape".into())]).await)?;
    denied(
        run(
            &read_only,
            "read-file",
            &[("path", "/data/../secret".into())],
        )
        .await,
    )?;
    assert_eq!(
        std::fs::read_to_string(root.path().join("file"))?,
        "retained"
    );
    grants.fs[0].write = true;
    let writable = driver(bytes, grants).await?;
    assert_eq!(
        run(
            &writable,
            "write-file",
            &[("path", "/data/file".into()), ("data", "changed".into())]
        )
        .await?,
        "written"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("file"))?,
        "changed"
    );
    Ok(())
}
#[tokio::test]
async fn tcp_requires_explicit_private_ip_and_dns_names_are_restricted() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        socket.write_all(b"K").await?;
        socket.shutdown().await
    });
    let domain = driver(
        bytes.clone(),
        HostGrants {
            tcp: vec![format!("localhost:{}", address.port())],
            ..Default::default()
        },
    )
    .await?;
    let addresses = run(&domain, "dns", &[("host", "localhost".into())]).await?;
    assert!(addresses.contains("127.0.0.1") || addresses.contains("::1"));
    denied(run(&domain, "dns", &[("host", "ungranted.invalid".into())]).await)?;
    denied(run(&domain, "tcp", &[("address", address.to_string().into())]).await)?;
    let literal = driver(
        bytes,
        HostGrants {
            tcp: vec![address.to_string()],
            ..Default::default()
        },
    )
    .await?;
    assert_eq!(
        run(&literal, "tcp", &[("address", address.to_string().into())]).await?,
        "K"
    );
    tokio::time::timeout(Duration::from_secs(2), server).await???;
    Ok(())
}
#[tokio::test]
async fn dns_rebinding_to_private_ip_is_denied_at_connect_in_a_real_component() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let mut factory = WasmTransport::load(
        bytes,
        HostGrants {
            tcp: vec![format!("rebind.test:{port}")],
            ..Default::default()
        },
    )
    .await?;
    let dns = Arc::new(grants::TestDns::new("rebind.test"));
    Arc::get_mut(&mut factory.factory)
        .ok_or("exclusive factory")?
        .test_dns = Some(dns.clone());
    let driver = PluginDriver::connect(Arc::new(factory)).await?;
    driver.initialize(Default::default()).await?;
    denied(
        run(
            &driver,
            "rebind",
            &[("host", "rebind.test".into()), ("port", port.into())],
        )
        .await,
    )?;
    assert!(dns.calls.load(Ordering::SeqCst) >= 2);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn wasi_http_origin_and_private_dns_checks_are_enforced_before_connect() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let allowed = driver(bytes.clone(), http_grant("http://example.invalid".into())).await?;
    denied(
        run(
            &allowed,
            "http",
            &[("url", format!("http://127.0.0.1:{port}/").into())],
        )
        .await,
    )?;
    let private = driver(bytes, http_grant(format!("http://localhost:{port}"))).await?;
    denied(
        run(
            &private,
            "http",
            &[("url", format!("http://localhost:{port}/").into())],
        )
        .await,
    )?;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn wasi_http_allows_an_explicit_origin_and_limits_response_body_and_headers() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let (origin, server) = server(response(b"allowed")).await?;
    let client = driver(bytes.clone(), http_grant(origin.clone())).await?;
    assert_eq!(
        run(&client, "http", &[("url", origin.into())]).await?,
        "allowed"
    );
    server.await??;
    let (origin, private_server) = self::server(response(b"private-authorized")).await?;
    let origin = origin.replace("127.0.0.1", "localhost");
    let mut private = http_grant(origin.clone());
    private.http.private_addresses = vec!["127.0.0.1".parse()?, "::1".parse()?];
    let client = driver(bytes.clone(), private).await?;
    assert_eq!(
        run(&client, "http", &[("url", origin.into())]).await?,
        "private-authorized"
    );
    private_server.await??;
    for headers in [false, true] {
        let (origin, server) = self::server(response(b"four")).await?;
        let mut grants = http_grant(origin.clone());
        if headers {
            grants.http.max_headers = 1;
        } else {
            grants.http.max_response_bytes = 2;
        }
        let limited = driver(bytes.clone(), grants).await?;
        let error = run(&limited, "http", &[("url", origin.into())])
            .await
            .err()
            .ok_or("response limit ignored")?;
        assert_eq!(
            error.lifecycle_error().ok_or("category")?.failure(),
            LifecycleFailure::CapacityExceeded
        );
        server.await??;
    }
    Ok(())
}
#[tokio::test]
async fn request_body_and_header_limits_prevent_network_side_effects() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    for headers in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let mut grants = http_grant(origin.clone());
        if headers {
            grants.http.max_headers = 1;
        } else {
            grants.http.max_request_bytes = 2;
        }
        let limited = driver(bytes.clone(), grants).await?;
        let error = run(
            &limited,
            "http",
            &[
                ("url", origin.into()),
                ("method", "POST".into()),
                ("bytes", 4.into()),
                ("headers", if headers { 4 } else { 0 }.into()),
            ],
        )
        .await
        .err()
        .ok_or("request limit ignored")?;
        assert_eq!(
            error.lifecycle_error().ok_or("category")?.failure(),
            LifecycleFailure::CapacityExceeded
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
    Ok(())
}
#[tokio::test]
async fn redirects_require_explicit_permission_and_are_rechecked() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    for enabled in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            read_request(&mut socket).await?;
            socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
            socket.shutdown().await?;
            if enabled {
                let (mut socket, _) = listener.accept().await?;
                read_request(&mut socket).await?;
                socket.write_all(&response(b"redirected")).await?;
                socket.shutdown().await?;
            }
            std::io::Result::Ok(())
        });
        let mut grants = http_grant(origin.clone());
        grants.http.allow_redirects = enabled;
        let client = driver(bytes.clone(), grants).await?;
        let result = run(&client, "http", &[("url", origin.into())]).await;
        if enabled {
            assert_eq!(result?, "redirected");
        } else {
            denied(result)?;
        }
        tokio::time::timeout(Duration::from_secs(2), server).await???;
    }
    let (target, target_server) = server(response(b"secret")).await?;
    let (origin,redirect)=server(format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()).await?;
    let mut grants = http_grant(origin.clone());
    grants.http.allow_redirects = true;
    let client = driver(bytes, grants).await?;
    denied(run(&client, "http", &[("url", origin.into())]).await)?;
    redirect.await??;
    assert!(!target_server.is_finished());
    target_server.abort();
    Ok(())
}
#[tokio::test]
async fn memory_fuel_and_wall_deadlines_abort_the_instance_and_allow_recovery() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    for mode in ["memory", "fuel", "timeout"] {
        let mut grants = HostGrants::default();
        grants.limits.control_instances = 1;
        match mode {
            "memory" => grants.limits.memory_bytes = 4 * 1024 * 1024,
            "fuel" => grants.limits.fuel = 500_000,
            _ => {
                grants.limits.fuel = u64::MAX;
                grants.limits.timeout_ms = 150;
            }
        }
        let factory = Arc::new(WasmTransport::load(bytes.clone(), grants).await?);
        let client = Arc::new(PluginDriver::connect(factory.clone()).await?);
        client.initialize(Default::default()).await?;
        let error = run(
            &client,
            if mode == "memory" { "memory" } else { "cpu" },
            &[],
        )
        .await
        .err()
        .ok_or("limit ignored")?;
        assert_eq!(
            error.lifecycle_error().ok_or("category")?.failure(),
            if mode == "timeout" {
                LifecycleFailure::TimedOut
            } else {
                LifecycleFailure::CapacityExceeded
            }
        );
        assert_eq!(
            run(&client, "echo", &[("data", "recovered".into())]).await?,
            "recovered"
        );
    }
    Ok(())
}
#[tokio::test]
async fn socket_deadline_releases_the_store_and_closes_the_remote_connection() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut byte = [0];
        socket.read(&mut byte).await
    });
    let mut grants = HostGrants {
        tcp: vec![address.to_string()],
        ..Default::default()
    };
    grants.limits.timeout_ms = 150;
    let client = driver(bytes, grants).await?;
    let error = run(&client, "tcp", &[("address", address.to_string().into())])
        .await
        .err()
        .ok_or("socket did not time out")?;
    assert_eq!(
        error.lifecycle_error().ok_or("category")?.failure(),
        LifecycleFailure::TimedOut
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), server).await???,
        0
    );
    assert_eq!(
        run(&client, "echo", &[("data", "fresh".into())]).await?,
        "fresh"
    );
    Ok(())
}
#[tokio::test]
async fn http_deadline_cancels_the_pending_connection() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        read_request(&mut socket).await?;
        let mut byte = [0];
        socket.read(&mut byte).await
    });
    let mut grants = http_grant(origin.clone());
    grants.http.timeout_ms = 80;
    let client = driver(bytes, grants).await?;
    let error = run(&client, "http", &[("url", origin.into())])
        .await
        .err()
        .ok_or("HTTP deadline ignored")?;
    assert_eq!(
        error.lifecycle_error().ok_or("category")?.failure(),
        LifecycleFailure::TimedOut
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), server).await???,
        0
    );
    Ok(())
}
#[tokio::test]
async fn components_only_and_configured_storage_concurrency_are_enforced() -> Result {
    let core = wat::parse_str("(module)")?;
    assert!(
        WasmTransport::load(core, HostGrants::default())
            .await
            .is_err()
    );
    let unknown = wat::parse_str("(component (import \"unknown:service/api\" (instance)))")?;
    assert!(
        WasmTransport::load(unknown, HostGrants::default())
            .await
            .is_err()
    );
    let Some(bytes) = component() else {
        return Ok(());
    };
    let mut grants = HostGrants::default();
    grants.limits.storage_instances = 1;
    let client = driver(bytes, grants).await?;
    let first = client
        .open(OpenContext::new(
            InstanceId::parse("one")?,
            Default::default(),
        ))
        .await?;
    let error = client
        .open(OpenContext::new(
            InstanceId::parse("two")?,
            Default::default(),
        ))
        .await
        .err()
        .ok_or("concurrency limit ignored")?;
    assert_eq!(
        error.lifecycle_error().ok_or("category")?.failure(),
        LifecycleFailure::CapacityExceeded
    );
    drop(first);
    // Resource::drop schedules Release on the runtime instead of blocking Drop.
    let id = InstanceId::parse("two")?;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match client
                .open(OpenContext::new(id.clone(), Default::default()))
                .await
            {
                Ok(fresh) => break Ok::<_, DriverError>(fresh),
                Err(error)
                    if error.lifecycle_error().is_some_and(|error| {
                        error.failure() == LifecycleFailure::CapacityExceeded
                    }) =>
                {
                    tokio::task::yield_now().await
                }
                Err(error) => break Err(error),
            }
        }
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn epoch_interrupts_real_guest_cpu_execution() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    let mut grants = HostGrants::default();
    grants.limits.fuel = u64::MAX;
    grants.limits.timeout_ms = 5_000;
    let epochs = grants.limits.timeout_ms.div_ceil(10);
    let factory = Arc::new(WasmTransport::load(bytes, grants).await?);
    factory.factory.ticker.abort();
    let engine = factory.factory.pre.engine().clone();
    let client = PluginDriver::connect(factory).await?;
    client.initialize(Default::default()).await?;
    let interrupt = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        for _ in 0..epochs {
            engine.increment_epoch();
        }
    });
    let error = run(&client, "cpu", &[])
        .await
        .err()
        .ok_or("epoch ignored")?;
    interrupt.await?;
    let error = error.lifecycle_error().ok_or("category")?;
    assert_eq!(error.failure(), LifecycleFailure::TimedOut);
    // An epoch trap carries its Wasmtime source; a wall deadline does not.
    assert!(std::error::Error::source(error).is_some());
    assert_eq!(
        run(&client, "echo", &[("data", "fresh".into())]).await?,
        "fresh"
    );
    Ok(())
}

#[tokio::test]
async fn cpu_budgets_do_not_replace_wall_deadlines_for_pending_socket_io() -> Result {
    let Some(bytes) = component() else {
        return Ok(());
    };
    for mode in ["epoch", "fuel"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let _ = accepted_tx.send(());
            let mut byte = [0];
            socket.read(&mut byte).await
        });
        let mut grants = HostGrants {
            tcp: vec![address.to_string()],
            ..Default::default()
        };
        grants.limits.timeout_ms = 5_000;
        grants.limits.fuel = if mode == "fuel" { 500_000 } else { u64::MAX };
        let epochs = grants.limits.timeout_ms.div_ceil(10);
        let factory = Arc::new(WasmTransport::load(bytes.clone(), grants).await?);
        factory.factory.ticker.abort();
        let engine = factory.factory.pre.engine().clone();
        let client = PluginDriver::connect(factory).await?;
        client.initialize(Default::default()).await?;
        let fields = [("address", address.to_string().into())];
        let mut pending = Box::pin(run(&client, "tcp", &fields));
        tokio::select! {
            result = &mut pending => return Err(format!("socket did not block: {result:?}").into()),
            accepted = tokio::time::timeout(Duration::from_secs(2), accepted_rx) => { accepted??; }
        }
        if mode == "epoch" {
            for _ in 0..epochs {
                engine.increment_epoch();
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut pending)
                .await
                .is_err()
        );
        drop(pending);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), server).await???,
            0
        );
        assert_eq!(
            run(&client, "echo", &[("data", "fresh".into())]).await?,
            "fresh"
        );
    }
    Ok(())
}
