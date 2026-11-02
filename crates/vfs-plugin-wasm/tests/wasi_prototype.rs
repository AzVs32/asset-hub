//! Real-component measurements used to decide whether the WASI migration can proceed.
use std::{path::PathBuf, time::Duration};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpHooks, WasiHttpView};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Default)]
struct Hooks;
impl WasiHttpHooks for Hooks {}
struct Host {
    wasi: WasiCtx,
    table: ResourceTable,
    http: WasiHttpCtx,
    hooks: Hooks,
}
impl WasiView for Host {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
impl WasiHttpView for Host {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.hooks,
        }
    }
}
fn runtime() -> Result<(Engine, Linker<Host>, Store<Host>)> {
    let mut config = Config::new();
    config.consume_fuel(true).epoch_interruption(true);
    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
    let mut builder = WasiCtx::builder();
    builder
        .env("PROTOTYPE_MARKER", "shared-context")
        .socket_addr_check(|addr, use_| {
            Box::pin(async move {
                use wasmtime_wasi::sockets::SocketAddrUse;
                matches!(use_, SocketAddrUse::TcpConnect) && addr.ip().is_loopback()
                    || matches!(use_, SocketAddrUse::TcpBind)
                        && addr.ip().is_unspecified()
                        && addr.port() == 0
            })
        })
        .allow_tcp(true);
    let mut store = Store::new(
        &engine,
        Host {
            wasi: builder.build(),
            table: ResourceTable::new(),
            http: WasiHttpCtx::new(),
            hooks: Hooks,
        },
    );
    store.set_fuel(2_000_000_000)?;
    store.set_epoch_deadline(1);
    store.fuel_async_yield_interval(Some(10_000))?;
    Ok((engine, linker, store))
}
fn rust_component() -> Option<PathBuf> {
    let path = std::env::var_os("VFS_WASI_PROTOTYPE_COMPONENT").map(PathBuf::from);
    if path.as_ref().is_none_or(|path| !path.is_file()) {
        eprintln!(
            "SKIP: build the real Rust component with scripts/build-wasi-prototype.sh and set VFS_WASI_PROTOTYPE_COMPONENT"
        );
        return None;
    }
    path
}
#[tokio::test]
async fn real_wasip2_rust_component_uses_the_shared_context() -> Result {
    let Some(path) = rust_component() else {
        return Ok(());
    };
    let (engine, linker, mut store) = runtime()?;
    let component = Component::from_binary(&engine, &std::fs::read(path)?)?;
    let instance = linker
        .instantiate_pre(&component)?
        .instantiate_async(&mut store)
        .await?;
    let call = instance.get_typed_func::<(Vec<u8>,), (Vec<u8>,)>(&mut store, "call")?;
    let (response,) = call.call_async(&mut store, (vec![],)).await?;
    assert_eq!(response, b"shared-context");
    Ok(())
}
#[tokio::test]
async fn measure_epoch_fuel_and_deadline_while_real_wasi_socket_read_is_blocked() -> Result {
    let Some(path) = rust_component() else {
        return Ok(());
    };
    for mode in ["epoch", "fuel", "timeout"] {
        let (engine, linker, mut store) = runtime()?;
        let component = Component::from_binary(&engine, &std::fs::read(&path)?)?;
        let instance = linker
            .instantiate_pre(&component)?
            .instantiate_async(&mut store)
            .await?;
        let call = instance.get_typed_func::<(Vec<u8>,), (Vec<u8>,)>(&mut store, "call")?;
        if mode == "fuel" {
            store.set_fuel(1_000_000)?;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let (mut socket, _) = listener.accept().await?;
            let _ = accepted_tx.send(());
            let mut byte = [0];
            let read = socket.read(&mut byte).await?;
            std::io::Result::Ok(read)
        });
        let mut pending =
            Box::pin(call.call_async(&mut store, (format!("read:{address}").into_bytes(),)));
        tokio::select! {
            result=&mut pending=>{return Err(format!("component returned before blocking on a real socket: {result:?}").into());},
            result=tokio::time::timeout(Duration::from_secs(5),accepted_rx)=>{result??;}
        }
        if mode == "epoch" {
            engine.increment_epoch();
        }
        let outcome = tokio::time::timeout(Duration::from_millis(150), &mut pending).await;
        assert!(
            outcome.is_err(),
            "CPU budgets do not terminate pending I/O; the wall deadline must fire"
        );
        eprintln!("PROTOTYPE c/{mode}: wall deadline elapsed");
        drop(pending);
        eprintln!("PROTOTYPE c/{mode}: remaining fuel={}", store.get_fuel()?);
        drop(store);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), server).await???,
            0
        );
    }
    Ok(())
}

#[tokio::test]
async fn fuel_and_epoch_do_interrupt_real_guest_cpu_execution() -> Result {
    let Some(path) = rust_component() else {
        return Ok(());
    };
    for mode in ["fuel", "epoch"] {
        let (engine, linker, mut store) = runtime()?;
        let component = Component::from_binary(&engine, &std::fs::read(&path)?)?;
        let instance = linker
            .instantiate_pre(&component)?
            .instantiate_async(&mut store)
            .await?;
        let call = instance.get_typed_func::<(Vec<u8>,), (Vec<u8>,)>(&mut store, "call")?;
        let ticker = if mode == "epoch" {
            Some(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                engine.increment_epoch();
            }))
        } else {
            store.set_fuel(1_000_000)?;
            None
        };
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            call.call_async(&mut store, (b"cpu".to_vec(),)),
        )
        .await?;
        let error = result.err().ok_or("CPU loop must trap")?;
        let trap = error
            .downcast_ref::<wasmtime::Trap>()
            .ok_or("expected Wasmtime trap")?;
        assert_eq!(
            *trap,
            if mode == "fuel" {
                wasmtime::Trap::OutOfFuel
            } else {
                wasmtime::Trap::Interrupt
            }
        );
        eprintln!("PROTOTYPE c/{mode}/cpu: {trap:?}");
        if let Some(ticker) = ticker {
            ticker.await?;
        }
    }
    Ok(())
}
