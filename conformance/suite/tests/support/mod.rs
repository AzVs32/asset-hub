use ah_vfs::{
    application::{dto::*, port::*},
    domain::*,
};
use std::{
    collections::HashMap,
    io::BufRead,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
};
use vfs_plugin_host::{PluginDriver, Transport};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const DEADLINE: Duration = Duration::from_secs(8);
pub const TOKEN: &str = "isolated-conformance-token";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Native,
    Grpc,
    Wasm,
}
pub const MODES: [Mode; 3] = [Mode::Native, Mode::Grpc, Mode::Wasm];
struct Slot {
    entered: Semaphore,
    released: Semaphore,
}
impl Default for Slot {
    fn default() -> Self {
        Self {
            entered: Semaphore::new(0),
            released: Semaphore::new(0),
        }
    }
}
type Slots = Arc<Mutex<HashMap<String, Arc<Slot>>>>;
fn slot(slots: &Slots, phase: &str) -> Arc<Slot> {
    slots
        .lock()
        .expect("fixture lock")
        .entry(phase.into())
        .or_insert_with(|| Arc::new(Slot::default()))
        .clone()
}
pub struct Gate {
    pub origin: String,
    slots: Slots,
    task: JoinHandle<()>,
}
impl Gate {
    async fn new() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let slots = Slots::default();
        let state = slots.clone();
        let task = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((mut socket, _)) = accepted else { break };
                        let state = state.clone();
                        tasks.spawn(async move {
                            let mut bytes = vec![];
                            let mut chunk = [0; 1024];
                            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                                let n = socket.read(&mut chunk).await?;
                                if n == 0 || bytes.len() > 8192 { return Ok::<_, std::io::Error>(()); }
                                bytes.extend_from_slice(&chunk[..n]);
                            }
                            let request = String::from_utf8_lossy(&bytes);
                            let phase = request.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/');
                            let barrier = slot(&state, phase);
                            barrier.entered.add_permits(1);
                            if let Ok(permit) = barrier.released.acquire().await { permit.forget(); }
                            let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                            Ok(())
                        });
                    }
                    _ = tasks.join_next(), if !tasks.is_empty() => {}
                }
            }
        });
        Ok(Self {
            origin,
            slots,
            task,
        })
    }
    pub async fn entered(&self, phase: &str) -> Result {
        let barrier = slot(&self.slots, phase);
        tokio::time::timeout(DEADLINE, barrier.entered.acquire())
            .await??
            .forget();
        Ok(())
    }
    pub fn release(&self, phase: &str) {
        slot(&self.slots, phase).released.add_permits(1);
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn artifact() -> PathBuf {
    std::env::var_os("CONFORMANCE_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/wasm32-unknown-unknown/release/conformance_wasm.wasm")
        })
}
pub struct Fixture {
    pub driver: Arc<dyn Driver>,
    pub transport: Option<Arc<dyn Transport>>,
    pub generation: String,
    pub gate: Gate,
    pub endpoint: Option<String>,
    child: Option<Process>,
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Fixture {
    pub async fn new(mode: Mode) -> Result<Self> {
        Self::start(mode, true).await
    }
    pub async fn uninitialized(mode: Mode) -> Result<Self> {
        Self::start(mode, false).await
    }
    async fn start(mode: Mode, initialize: bool) -> Result<Self> {
        let gate = Gate::new().await?;
        let mut child = None;
        let mut endpoint = None;
        let transport: Option<Arc<dyn Transport>> = match mode {
            Mode::Native => None,
            Mode::Grpc => {
                let mut process = Process(
                    Command::new(env!("CARGO_BIN_EXE_conformance-grpc"))
                        .env("CONFORMANCE_TOKEN", TOKEN)
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()?,
                );
                let stdout = process.0.stdout.take().ok_or("missing stdout")?;
                let address = tokio::time::timeout(
                    DEADLINE,
                    tokio::task::spawn_blocking(move || {
                        std::io::BufReader::new(stdout).lines().next().transpose()
                    }),
                )
                .await???
                .ok_or("plugin exited before listening")?;
                let url = format!("http://{address}");
                let transport = vfs_plugin_grpc::GrpcTransport::connect(url.clone(), TOKEN).await?;
                endpoint = Some(url);
                child = Some(process);
                Some(Arc::new(transport))
            }
            Mode::Wasm => Some(Arc::new(
                vfs_plugin_wasm::WasmTransport::load(
                    tokio::fs::read(artifact()).await?,
                    vec![gate.origin.clone()],
                )
                .await?,
            )),
        };
        let mut generation = String::new();
        let driver: Arc<dyn Driver> = if let Some(transport) = &transport {
            let reply = transport.call("", Op::Describe, vec![]).await?;
            let Value::Description { description } = reply.value else {
                return Err("description missing".into());
            };
            generation = description.generation;
            Arc::new(PluginDriver::connect(transport.clone()).await?)
        } else {
            Arc::new(conformance_native::ConformanceDriver::new())
        };
        if initialize {
            driver.initialize(Default::default()).await?;
        }
        Ok(Self {
            driver,
            transport,
            generation,
            gate,
            endpoint,
            child,
        })
    }
    pub fn config(&self, profile: &str) -> InstanceConfig {
        [
            ("profile".into(), profile.into()),
            ("gate".into(), self.gate.origin.clone().into()),
        ]
        .into()
    }
    pub async fn open(&self, profile: &str) -> Result<Arc<dyn Storage>> {
        let storage = self
            .driver
            .open(OpenContext::host_managed(
                InstanceId::parse("instance")?,
                self.config(profile),
            ))
            .await?;
        validate_storage_capabilities(self.driver.info().capabilities(), storage.as_ref())?;
        Ok(storage)
    }
    pub async fn raw(&self, operation: Op) -> Result<p::Reply> {
        Ok(self
            .transport
            .as_ref()
            .ok_or("native mode has no wire transport")?
            .call(&self.generation, operation, vec![])
            .await?)
    }
    pub async fn kill(&mut self) -> Result {
        let process = &mut self.child.as_mut().ok_or("not a gRPC fixture")?.0;
        process.kill()?;
        process.wait()?;
        Ok(())
    }
}

pub fn path(path: &str) -> StoragePath {
    StoragePath::parse(path).expect("fixture path")
}
pub async fn stat(
    storage: &dyn Storage,
    name: &str,
) -> std::result::Result<StorageEntry, StorageError> {
    storage
        .stat_entry(StorageStatEntryRequest { path: path(name) })
        .await
}
pub async fn metric(storage: &dyn Storage, name: &str) -> Result<u64> {
    Ok(stat(storage, &format!("__stats-{name}"))
        .await?
        .size
        .ok_or("metric has no value")?)
}
pub async fn eventually_metric(storage: &dyn Storage, name: &str, expected: u64) -> Result {
    let mut observed = None;
    let outcome = tokio::time::timeout(DEADLINE, async {
        loop {
            let actual = metric(storage, name).await?;
            observed = Some(actual);
            if actual == expected {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    outcome
        .map_err(|_| format!("metric {name}: expected {expected}, last observed {observed:?}"))??;
    Ok(())
}
pub async fn read(storage: &dyn Storage, name: &str) -> Result<Vec<u8>> {
    let mut reader = storage
        .read_file(StorageReadFileRequest::new(path(name)))
        .await?;
    let mut bytes = vec![];
    while let Some(chunk) = reader.next_chunk().await? {
        assert!(!chunk.is_empty());
        bytes.extend(chunk);
    }
    assert!(reader.next_chunk().await?.is_none());
    Ok(bytes)
}
pub async fn writer(storage: &dyn Storage, name: &str) -> Result<Box<dyn StorageFileWriter>> {
    Ok(storage
        .write_file(StorageWriteFileRequest {
            path: path(name),
            mode: StorageWriteFileMode::CreateNew,
            require_atomic_publish: true,
        })
        .await?)
}
pub fn kind<T>(result: std::result::Result<T, StorageError>) -> StorageErrorKind {
    result.err().expect("expected storage error").kind()
}
pub fn category(error: &(dyn std::error::Error + 'static)) -> Option<StorageErrorKind> {
    if let Some(error) = error.downcast_ref::<StorageError>() {
        return Some(error.kind());
    }
    if let Some(error) = error.downcast_ref::<DriverError>() {
        return error.storage_error().map(StorageError::kind);
    }
    error.source().and_then(category)
}
