//! Portable WebDAV driver using only the service.v1 HTTP import.
//!
//! Instance records and credential encryption belong to the host. Authentication
//! is anonymous, Basic or Bearer. The endpoint is an existing collection URL.
//! Staged PUT writes are bounded to max_write_bytes (at most 8 MiB), matching the
//! current bounded HTTP import. Larger reads require server byte-range support.
//! Atomic publication/rename, POSIX permissions and xattrs are not advertised.
//! Removal is not advertised or implemented: RFC 4918 collection DELETE is
//! recursive and cannot guarantee the VFS empty-directory removal contract.
#![cfg(any(target_arch = "wasm32", test))]

mod client;
mod multistatus;

use std::{cell::{Cell, RefCell}, collections::BTreeMap};
use vfs_plugin_protocol::{self as p, Value};

wit_bindgen::generate!({ path: "../../crates/vfs-plugin-protocol/wit", world: "plugin" });
use assethub::vfs::http;
use exports::assethub::vfs::driver::{self as api, Guest, GuestReader, GuestStorage, GuestWriter};

type Result<T> = std::result::Result<T, api::Failure>;
const CHUNK_SIZE: u32 = 64 * 1024;

fn failure(code: &str) -> api::Failure {
    api::Failure { code: code.into(), field: None, config_kind: None }
}
fn invalid(field: &str, kind: &str) -> api::Failure {
    api::Failure { code: "invalid_instance_config".into(), field: Some(field.into()), config_kind: Some(kind.into()) }
}
fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| failure("invalid_data"))?;
    if bytes.len() > p::MAX_CONTROL_BYTES { return Err(failure("too_large")); }
    Ok(bytes)
}
fn object(bytes: &[u8]) -> Result<p::Object> {
    if bytes.len() > p::MAX_CONTROL_BYTES { return Err(failure("too_large")); }
    serde_json::from_slice(bytes).map_err(|_| failure("invalid_request"))
}
fn path(value: &str) -> Result<()> {
    if value.len() > 4096 || value.starts_with('/') || value.contains(['\\', ':', '\0'])
        || (!value.is_empty() && value.split('/').any(|part| matches!(part, "" | "." | ".."))) {
        return Err(failure("invalid_path"));
    }
    Ok(())
}
fn capabilities() -> Vec<String> {
    ["stat", "list", "range_read", "write_new", "write_replace", "hierarchical", "empty_dirs", "create_dir"]
        .into_iter().map(str::to_owned).collect()
}

struct WebdavDriver;
impl Guest for WebdavDriver {
    type Storage = WebdavStorage;
    type Reader = WebdavReader;
    type Writer = WebdavWriter;

    fn describe() -> Vec<u8> {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "endpoint": {"type": "string", "x-immutable": true},
                "username": {"type": "string", "x-secret": true},
                "password": {"type": "string", "x-secret": true},
                "bearer_token": {"type": "string", "x-secret": true},
                "max_write_bytes": {"type": "integer", "minimum": 1, "maximum": p::MAX_DATA_BYTES, "default": p::MAX_DATA_BYTES, "x-immutable": true}
            },
            "required": ["endpoint"],
            "additionalProperties": false
        });
        let instance_schema = schema.as_object().map(|map| map.clone().into_iter().collect());
        encode(&p::Description {
            major: p::PROTOCOL_MAJOR,
            minor: p::PROTOCOL_MINOR,
            required_features: vec!["instances.host-managed.v1".into(), p::INSTANCE_SCHEMA_PROFILE.into()],
            features: p::PROTOCOL_FEATURES.iter().map(|feature| (*feature).into()).collect(),
            required_host_capabilities: vec![p::HOST_HTTP.into()],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: String::new(),
            key: "webdav".into(),
            name: "WebDAV".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            child_mounts: true,
            capabilities: capabilities(),
            ownership: p::Ownership::Host,
            settings_schema: None,
            instance_schema,
            config_version: 1,
        }).unwrap_or_default()
    }
    fn initialize(settings: Vec<u8>) -> Result<()> {
        if object(&settings)?.is_empty() { Ok(()) } else { Err(failure("unsupported_operation")) }
    }
    fn prepare(config: Vec<u8>) -> Result<Vec<u8>> {
        let mut config = object(&config)?;
        let client = client::Client::parse(&config)?;
        config.insert("endpoint".into(), client.endpoint.as_str().into());
        config.insert("max_write_bytes".into(), client.max_write_bytes.into());
        encode(&Value::Prepared {
            config,
            details: BTreeMap::from([
                ("endpoint".into(), client.endpoint.as_str().into()),
                ("max_write_bytes".into(), client.max_write_bytes.into()),
            ]),
        })
    }
    fn migrate(from_version: u32, config: Vec<u8>) -> Result<Vec<u8>> {
        if from_version != 1 { return Err(failure("unsupported_operation")); }
        let fields = object(&config)?;
        client::Client::parse(&fields)?;
        encode(&fields)
    }
    fn test_connection(config: Vec<u8>) -> Result<()> {
        client::Client::parse(&object(&config)?)?.check_root()
    }
    fn instances(_control: Vec<u8>) -> Result<Vec<u8>> { Err(failure("unsupported_operation")) }
    fn open(_id: String, config: Option<Vec<u8>>) -> Result<api::Storage> {
        let config = config.ok_or_else(|| failure("invalid_request"))?;
        let client = client::Client::parse(&object(&config)?)?;
        client.check_root()?;
        Ok(api::Storage::new(WebdavStorage { client }))
    }
}

struct WebdavStorage { client: client::Client }
impl GuestStorage for WebdavStorage {
    fn info(&self) -> Vec<u8> {
        encode(&p::StorageInfo { capabilities: capabilities(), chunk_size: CHUNK_SIZE }).unwrap_or_default()
    }
    fn call(&self, control: Vec<u8>, data: Vec<u8>) -> Result<api::Reply> {
        if control.len() > p::MAX_CONTROL_BYTES { return Err(failure("too_large")); }
        if !data.is_empty() { return Err(failure("invalid_request")); }
        let request: p::StorageOp = serde_json::from_slice(&control).map_err(|_| failure("invalid_request"))?;
        let value = match request {
            p::StorageOp::Stat { path } => Value::Entry { entry: self.client.stat(&path)?.entry },
            p::StorageOp::List { path, after, limit } => {
                let (entries, next_after) = self.client.list(&path, after.as_deref(), limit)?;
                Value::Page { entries, next_after }
            }
            p::StorageOp::CreateDir { path, recursive } => Value::Entry { entry: self.client.create_dir(&path, recursive)? },
            _ => return Err(failure("unsupported_operation")),
        };
        Ok(api::Reply { control: encode(&value)?, data: vec![] })
    }
    fn read(&self, path: String, offset: u64, length: Option<u64>) -> Result<api::Reader> {
        let metadata = self.client.stat(&path)?;
        if metadata.entry.kind == p::EntryKind::Dir { return Err(failure("is_dir")); }
        if metadata.entry.kind != p::EntryKind::File { return Err(failure("unsupported_operation")); }
        let size = metadata.entry.size.ok_or_else(|| failure("unsupported_operation"))?;
        Ok(api::Reader::new(WebdavReader {
            session: RefCell::new(Some(ReadSession {
                client: self.client.clone(), path, offset,
                remaining: length.unwrap_or(u64::MAX).min(size.saturating_sub(offset)),
                size, etag: metadata.strong_etag(), cached: None,
            })),
            eof: Cell::new(false),
        }))
    }
    fn write(&self, path: String, replace: bool, atomic: bool) -> Result<api::Writer> {
        if atomic { return Err(failure("unsupported_operation")); }
        let condition = self.client.write_condition(&path, replace)?;
        Ok(api::Writer::new(WebdavWriter {
            session: RefCell::new(Some(WriteSession { client: self.client.clone(), path, condition, bytes: vec![] })),
        }))
    }
}

struct ReadSession {
    client: client::Client, path: String, offset: u64, remaining: u64, size: u64,
    etag: Option<String>, cached: Option<Vec<u8>>,
}
struct WebdavReader { session: RefCell<Option<ReadSession>>, eof: Cell<bool> }
impl GuestReader for WebdavReader {
    fn next(&self) -> Result<Option<Vec<u8>>> {
        if self.eof.get() { return Ok(None); }
        // Take before I/O so errors cannot silently resume at an uncertain offset.
        let mut session = self.session.borrow_mut().take().ok_or_else(|| failure("session_closed"))?;
        if session.remaining == 0 { self.eof.set(true); return Ok(None); }
        let length = session.remaining.min(u64::from(CHUNK_SIZE));
        let data = if let Some(bytes) = &session.cached {
            let start = usize::try_from(session.offset).map_err(|_| failure("invalid_data"))?;
            let end = start.checked_add(length as usize).ok_or_else(|| failure("invalid_data"))?;
            bytes.get(start..end).ok_or_else(|| failure("invalid_data"))?.to_vec()
        } else {
            let (data, full) = session.client.read_range(&session.path, session.offset, length, session.size, session.etag.as_deref())?;
            session.cached = full;
            data
        };
        if data.is_empty() || data.len() as u64 > length { return Err(failure("invalid_data")); }
        session.offset = session.offset.checked_add(data.len() as u64).ok_or_else(|| failure("invalid_data"))?;
        session.remaining -= data.len() as u64;
        *self.session.borrow_mut() = Some(session);
        Ok(Some(data))
    }
}

struct WriteSession { client: client::Client, path: String, condition: client::WriteCondition, bytes: Vec<u8> }
struct WebdavWriter { session: RefCell<Option<WriteSession>> }
impl GuestWriter for WebdavWriter {
    fn write(&self, data: Vec<u8>) -> Result<()> {
        let mut session = self.session.borrow_mut().take().ok_or_else(|| failure("session_closed"))?;
        let length = session.bytes.len().checked_add(data.len()).ok_or_else(|| failure("too_large"))?;
        if length > session.client.max_write_bytes { return Err(failure("too_large")); }
        session.bytes.try_reserve_exact(data.len()).map_err(|_| failure("capacity_exceeded"))?;
        session.bytes.extend_from_slice(&data);
        *self.session.borrow_mut() = Some(session);
        Ok(())
    }
    fn commit(&self) -> Result<Vec<u8>> {
        // Both success and failure are terminal; no remote request is retried.
        let session = self.session.borrow_mut().take().ok_or_else(|| failure("session_closed"))?;
        encode(&session.client.put_file(&session.path, session.condition, session.bytes)?)
    }
    fn abort(&self) -> Result<()> {
        self.session.borrow_mut().take().ok_or_else(|| failure("session_closed"))?;
        Ok(())
    }
}
#[cfg(target_arch = "wasm32")]
export!(WebdavDriver);

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> Result<WebdavStorage> {
        let config = p::Object::from([("endpoint".into(), "https://webdav.invalid/root/".into())]);
        Ok(WebdavStorage {
            client: client::Client::parse(&config)?,
        })
    }

    #[test]
    fn driver_and_storage_do_not_advertise_partial_remove_support() -> Result<()> {
        let description: p::Description = serde_json::from_slice(&WebdavDriver::describe())
            .map_err(|_| failure("invalid_data"))?;
        let info: p::StorageInfo =
            serde_json::from_slice(&storage()?.info()).map_err(|_| failure("invalid_data"))?;
        assert_eq!(description.capabilities, info.capabilities);
        assert!(
            !description
                .capabilities
                .iter()
                .any(|capability| capability == "remove")
        );
        for capability in ["hierarchical", "empty_dirs", "create_dir"] {
            assert!(
                description
                    .capabilities
                    .iter()
                    .any(|actual| actual == capability)
            );
        }
        Ok(())
    }

    #[test]
    fn remove_is_rejected_without_http_for_every_entry_kind() -> Result<()> {
        let storage = storage()?;
        // Native WIT HTTP imports panic if called, so this also verifies that
        // unsupported removal does not inspect or mutate remote resources.
        for path in ["", "file", "empty-dir", "nonempty-dir", "missing"] {
            let control = encode(&p::StorageOp::Remove { path: path.into() })?;
            let error = storage
                .call(control, vec![])
                .err()
                .ok_or_else(|| failure("invalid_data"))?;
            assert_eq!(error.code, "unsupported_operation", "path: {path}");
        }
        Ok(())
    }
}
