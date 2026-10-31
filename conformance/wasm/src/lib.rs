//! Component Model implementation of the same fixture, using public WIT only.
#![cfg(target_arch = "wasm32")]
use conformance_model as m;
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};
use vfs_plugin_protocol::{self as p, Value};
wit_bindgen::generate!({ path: "../../crates/vfs-plugin-protocol/wit", world: "plugin" });
use assethub::vfs::{clock, http, random};
use exports::assethub::vfs::driver::{self as api, Guest, GuestReader, GuestStorage, GuestWriter};
type Result<T> = std::result::Result<T, api::Failure>;
fn failure(e: p::Failure) -> api::Failure {
    api::Failure {
        code: e.code,
        field: e.field,
        config_kind: e.config_kind,
    }
}
fn bad(code: &str) -> api::Failure {
    failure(m::failure(code))
}
fn json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| bad("invalid_data"))
}
fn config(bytes: &[u8]) -> Result<p::Object> {
    serde_json::from_slice(bytes).map_err(|_| bad("invalid_request"))
}
fn wait(config: &m::Config, phase: &str) -> Result<()> {
    let origin = config.gate.as_ref().ok_or_else(|| bad("invalid_request"))?;
    let reply = http::request_http(&http::Request {
        method: "GET".into(),
        url: format!("{origin}/{phase}"),
        headers: vec![],
        body: vec![],
    })
    .map_err(|code| bad(&code))?;
    if reply.status != 200 {
        return Err(bad("backend"));
    }
    Ok(())
}
thread_local! { static WORLD: Arc<m::World> = m::World::new(); }
struct ConformanceDriver;
impl Guest for ConformanceDriver {
    type Storage = WasmStorage;
    type Reader = WasmReader;
    type Writer = WasmWriter;
    fn describe() -> Vec<u8> {
        let mut d = m::description();
        d.required_host_capabilities = p::WASM_HOST_CAPABILITIES
            .iter()
            .map(|s| (*s).into())
            .collect();
        json(&d).expect("fixture declaration")
    }
    fn initialize(settings: Vec<u8>) -> Result<()> {
        if !config(&settings)?.is_empty() {
            return Err(bad("unsupported_operation"));
        }
        let _ = clock::unix_time();
        let entropy = random::random_bytes(16).map_err(|code| bad(&code))?;
        if entropy.len() != 16 || random::random_bytes(65_537).is_ok() {
            return Err(bad("invalid_data"));
        }
        Ok(())
    }
    fn prepare(bytes: Vec<u8>) -> Result<Vec<u8>> {
        let config = config(&bytes)?;
        m::prepare(&config).map_err(failure)?;
        json(&Value::Prepared {
            config,
            details: Default::default(),
        })
    }
    fn migrate(from: u32, bytes: Vec<u8>) -> Result<Vec<u8>> {
        if from != 1 {
            return Err(bad("unsupported_operation"));
        }
        let config = config(&bytes)?;
        m::prepare(&config).map_err(failure)?;
        json(&config)
    }
    fn test_connection(bytes: Vec<u8>) -> Result<()> {
        m::prepare(&config(&bytes)?).map(|_| ()).map_err(failure)
    }
    fn instances(_control: Vec<u8>) -> Result<Vec<u8>> {
        Err(bad("unsupported_operation"))
    }
    fn open(id: String, bytes: Option<Vec<u8>>) -> Result<api::Storage> {
        let config =
            m::prepare(&config(&bytes.ok_or_else(|| bad("invalid_request"))?)?).map_err(failure)?;
        if id == "wait-open" {
            wait(&config, "open")?;
        }
        Ok(api::Storage::new(WasmStorage(
            WORLD.with(|world| m::Storage::new(world.clone(), config)),
        )))
    }
}
struct WasmStorage(m::Storage);
impl Drop for WasmStorage {
    fn drop(&mut self) {
        if self.0.config.profile == "drop-trap" {
            panic!("conformance injected storage destructor trap");
        }
    }
}
impl GuestStorage for WasmStorage {
    fn info(&self) -> Vec<u8> {
        if self.0.config.profile == "malformed-info" {
            return b"{".to_vec();
        }
        json(&self.0.info()).expect("fixture storage info")
    }
    fn call(&self, control: Vec<u8>, _data: Vec<u8>) -> Result<api::Reply> {
        let request: p::StorageOp =
            serde_json::from_slice(&control).map_err(|_| bad("invalid_request"))?;
        let value = match request {
            p::StorageOp::Stat { path } => Value::Entry {
                entry: self.0.stat(&path).map_err(failure)?,
            },
            p::StorageOp::List { path, after, limit } => {
                let (entries, next_after) = self
                    .0
                    .list(&path, after.as_deref(), limit)
                    .map_err(failure)?;
                Value::Page {
                    entries,
                    next_after,
                }
            }
            p::StorageOp::Remove { path } => {
                self.0.remove(&path).map_err(failure)?;
                Value::Unit
            }
            _ => return Err(bad("unsupported_operation")),
        };
        Ok(api::Reply {
            control: json(&value)?,
            data: vec![],
        })
    }
    fn read(&self, path: String, offset: u64, length: Option<u64>) -> Result<api::Reader> {
        Ok(api::Reader::new(WasmReader {
            inner: RefCell::new(Some(self.0.read(&path, offset, length).map_err(failure)?)),
            config: self.0.config.clone(),
            block: Cell::new(path == "wait-read"),
            eof: Cell::new(false),
        }))
    }
    fn write(&self, path: String, replace: bool, _atomic: bool) -> Result<api::Writer> {
        Ok(api::Writer::new(WasmWriter {
            inner: RefCell::new(Some(self.0.write(&path, replace).map_err(failure)?)),
            config: self.0.config.clone(),
            trap_on_drop: path == "drop-trap",
        }))
    }
}
struct WasmReader {
    inner: RefCell<Option<m::Reader>>,
    config: m::Config,
    block: Cell<bool>,
    eof: Cell<bool>,
}
impl GuestReader for WasmReader {
    fn next(&self) -> Result<Option<Vec<u8>>> {
        if self.eof.get() {
            return Ok(None);
        }
        let mut reader = self
            .inner
            .borrow_mut()
            .take()
            .ok_or_else(|| bad("session_closed"))?;
        if self.block.replace(false) {
            wait(&self.config, "read")?;
        }
        let chunk = reader.next_chunk().map_err(failure)?;
        if chunk.is_some() {
            *self.inner.borrow_mut() = Some(reader);
        } else {
            self.eof.set(true);
        }
        Ok(chunk)
    }
}
struct WasmWriter {
    inner: RefCell<Option<m::Writer>>,
    config: m::Config,
    trap_on_drop: bool,
}
impl Drop for WasmWriter {
    fn drop(&mut self) {
        if self.trap_on_drop {
            panic!("conformance injected writer destructor trap");
        }
    }
}
impl GuestWriter for WasmWriter {
    fn write(&self, bytes: Vec<u8>) -> Result<()> {
        let mut writer = self
            .inner
            .borrow_mut()
            .take()
            .ok_or_else(|| bad("session_closed"))?;
        if writer.path == "wait-write" {
            wait(&self.config, "write")?;
        }
        writer.write(&bytes).map_err(failure)?;
        *self.inner.borrow_mut() = Some(writer);
        Ok(())
    }
    fn commit(&self) -> Result<Vec<u8>> {
        let pending = self
            .inner
            .borrow_mut()
            .take()
            .ok_or_else(|| bad("session_closed"))?
            .take_commit()
            .map_err(failure)?;
        if pending.path == "wait-commit" {
            wait(&self.config, "commit")?;
        }
        let after_publish = pending.path == "wait-reply";
        let entry = pending.publish().map_err(failure)?;
        if after_publish {
            wait(&self.config, "reply")?;
        }
        json(&entry)
    }
    fn abort(&self) -> Result<()> {
        let mut writer = self
            .inner
            .borrow_mut()
            .take()
            .ok_or_else(|| bad("session_closed"))?;
        let block = writer.path == "wait-abort";
        writer.abort().map_err(failure)?;
        if block {
            wait(&self.config, "abort")?;
        }
        Ok(())
    }
}
export!(ConformanceDriver);
