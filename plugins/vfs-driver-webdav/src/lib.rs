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

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use vfs_plugin_protocol::{self as p, Value};

vfs_plugin_guest::export_plugin!(WebdavDriver, "../../crates/vfs-plugin-protocol/wit");
use assethub::vfs::http;
use vfs_plugin_guest::api::{self, Guest, GuestReader, GuestStorage, GuestWriter};

type Result<T> = std::result::Result<T, api::Failure>;
const CHUNK_SIZE: u32 = 64 * 1024;

fn failure(code: p::FailureCode) -> api::Failure {
    api::Failure {
        code: code as i32,
        field: None,
        config_kind: None,
    }
}
fn invalid(field: &str, kind: p::ConfigErrorKind) -> api::Failure {
    api::Failure {
        code: p::FailureCode::InvalidInstanceConfig as i32,
        field: Some(field.into()),
        config_kind: Some(kind as i32),
    }
}
fn path(value: &str) -> Result<()> {
    if value.len() > 4096
        || value.starts_with('/')
        || value.contains(['\\', ':', '\0'])
        || (!value.is_empty() && value.split('/').any(|part| matches!(part, "" | "." | "..")))
    {
        return Err(failure(p::FailureCode::InvalidPath));
    }
    Ok(())
}
fn capabilities() -> Vec<i32> {
    [
        p::Capability::Stat,
        p::Capability::List,
        p::Capability::RangeRead,
        p::Capability::WriteNew,
        p::Capability::WriteReplace,
        p::Capability::Hierarchical,
        p::Capability::EmptyDirs,
        p::Capability::CreateDir,
    ]
    .into_iter()
    .map(Into::into)
    .collect()
}

struct WebdavDriver;
impl Guest for WebdavDriver {
    fn describe() -> p::Description {
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
        let instance_schema = schema
            .as_object()
            .map(|map| map.clone().into_iter().collect::<p::JsonObject>());
        p::Description {
            major: p::PROTOCOL_MAJOR,
            minor: p::PROTOCOL_MINOR,
            required_features: vec![
                "instances.host-managed.v1".into(),
                p::INSTANCE_SCHEMA_PROFILE.into(),
            ],
            features: p::PROTOCOL_FEATURES
                .iter()
                .map(|feature| (*feature).into())
                .collect(),
            required_host_capabilities: vec![p::HOST_HTTP.into()],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: String::new(),
            key: "webdav".into(),
            name: "WebDAV".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            child_mounts: true,
            capabilities: capabilities(),
            ownership: p::Ownership::Host as i32,
            settings_schema: None,
            instance_schema: instance_schema.map(Into::into),
            config_version: 1,
        }
    }
    fn initialize(settings: p::Object) -> Result<()> {
        if settings.fields.is_empty() {
            Ok(())
        } else {
            Err(failure(p::FailureCode::UnsupportedOperation))
        }
    }
    fn prepare(config: p::Object) -> Result<p::Prepared> {
        let mut config = p::json_object(config)?;
        let client = client::Client::parse(&config)?;
        config.insert("endpoint".into(), client.endpoint.as_str().into());
        config.insert("max_write_bytes".into(), client.max_write_bytes.into());
        Ok(p::Prepared {
            config: Some(config.into()),
            details: Some(
                BTreeMap::from([
                    ("endpoint".into(), client.endpoint.as_str().into()),
                    ("max_write_bytes".into(), client.max_write_bytes.into()),
                ])
                .into(),
            ),
        })
    }
    fn migrate(from_version: u32, config: p::Object) -> Result<p::Object> {
        if from_version != 1 {
            return Err(failure(p::FailureCode::UnsupportedOperation));
        }
        let fields = p::json_object(config)?;
        client::Client::parse(&fields)?;
        Ok(fields.into())
    }
    fn test_connection(config: p::Object) -> Result<()> {
        client::Client::parse(&p::json_object(config)?)?.check_root()
    }
    fn open(_id: String, config: Option<p::Object>) -> Result<api::Storage> {
        let config =
            p::json_object(config.ok_or_else(|| failure(p::FailureCode::InvalidRequest))?)?;
        let client = client::Client::parse(&config)?;
        client.check_root()?;
        Ok(api::Storage::new(WebdavStorage { client }))
    }
}

struct WebdavStorage {
    client: client::Client,
}
impl GuestStorage for WebdavStorage {
    fn info(&self) -> p::StorageInfo {
        p::StorageInfo {
            capabilities: capabilities(),
            chunk_size: CHUNK_SIZE,
        }
    }
    fn call(&self, request: p::StorageOp, data: Vec<u8>) -> Result<p::Reply> {
        if !data.is_empty() {
            return Err(failure(p::FailureCode::InvalidRequest));
        }
        let value = match request {
            p::StorageOp::Stat(p::Path { path }) => Value::Entry(self.client.stat(&path)?.entry),
            p::StorageOp::List(p::List { path, after, limit }) => {
                let (entries, next_after) = self.client.list(&path, after.as_deref(), limit)?;
                Value::Page(p::Page {
                    entries,
                    next_after,
                })
            }
            p::StorageOp::CreateDir(p::CreateDir { path, recursive }) => {
                Value::Entry(self.client.create_dir(&path, recursive)?)
            }
            _ => return Err(failure(p::FailureCode::UnsupportedOperation)),
        };
        Ok(p::Reply::new(value))
    }
    fn read(&self, path: String, offset: u64, length: Option<u64>) -> Result<api::Reader> {
        let metadata = self.client.stat(&path)?;
        if metadata.entry.kind == p::EntryKind::Dir as i32 {
            return Err(failure(p::FailureCode::IsDir));
        }
        if metadata.entry.kind != p::EntryKind::File as i32 {
            return Err(failure(p::FailureCode::UnsupportedOperation));
        }
        let size = metadata
            .entry
            .size
            .ok_or_else(|| failure(p::FailureCode::UnsupportedOperation))?;
        Ok(api::Reader::new(WebdavReader {
            session: RefCell::new(Some(ReadSession {
                client: self.client.clone(),
                path,
                offset,
                remaining: length.unwrap_or(u64::MAX).min(size.saturating_sub(offset)),
                size,
                etag: metadata.strong_etag(),
                cached: None,
            })),
            eof: Cell::new(false),
        }))
    }
    fn write(&self, path: String, replace: bool, atomic: bool) -> Result<api::Writer> {
        if atomic {
            return Err(failure(p::FailureCode::UnsupportedOperation));
        }
        let condition = self.client.write_condition(&path, replace)?;
        Ok(api::Writer::new(WebdavWriter {
            session: RefCell::new(Some(WriteSession {
                client: self.client.clone(),
                path,
                condition,
                bytes: vec![],
            })),
        }))
    }
}

struct ReadSession {
    client: client::Client,
    path: String,
    offset: u64,
    remaining: u64,
    size: u64,
    etag: Option<String>,
    cached: Option<Vec<u8>>,
}
struct WebdavReader {
    session: RefCell<Option<ReadSession>>,
    eof: Cell<bool>,
}
impl GuestReader for WebdavReader {
    fn next(&self) -> Result<Option<Vec<u8>>> {
        if self.eof.get() {
            return Ok(None);
        }
        // Take before I/O so errors cannot silently resume at an uncertain offset.
        let mut session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| failure(p::FailureCode::SessionClosed))?;
        if session.remaining == 0 {
            self.eof.set(true);
            return Ok(None);
        }
        let length = session.remaining.min(u64::from(CHUNK_SIZE));
        let data = if let Some(bytes) = &session.cached {
            let start = usize::try_from(session.offset)
                .map_err(|_| failure(p::FailureCode::InvalidData))?;
            let end = start
                .checked_add(length as usize)
                .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
            bytes
                .get(start..end)
                .ok_or_else(|| failure(p::FailureCode::InvalidData))?
                .to_vec()
        } else {
            let (data, full) = session.client.read_range(
                &session.path,
                session.offset,
                length,
                session.size,
                session.etag.as_deref(),
            )?;
            session.cached = full;
            data
        };
        if data.is_empty() || data.len() as u64 > length {
            return Err(failure(p::FailureCode::InvalidData));
        }
        session.offset = session
            .offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
        session.remaining -= data.len() as u64;
        *self.session.borrow_mut() = Some(session);
        Ok(Some(data))
    }
}

struct WriteSession {
    client: client::Client,
    path: String,
    condition: client::WriteCondition,
    bytes: Vec<u8>,
}
struct WebdavWriter {
    session: RefCell<Option<WriteSession>>,
}
impl GuestWriter for WebdavWriter {
    fn write(&self, data: Vec<u8>) -> Result<()> {
        let mut session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| failure(p::FailureCode::SessionClosed))?;
        let length = session
            .bytes
            .len()
            .checked_add(data.len())
            .ok_or_else(|| failure(p::FailureCode::TooLarge))?;
        if length > session.client.max_write_bytes {
            return Err(failure(p::FailureCode::TooLarge));
        }
        session
            .bytes
            .try_reserve_exact(data.len())
            .map_err(|_| failure(p::FailureCode::CapacityExceeded))?;
        session.bytes.extend_from_slice(&data);
        *self.session.borrow_mut() = Some(session);
        Ok(())
    }
    fn commit(&self) -> Result<p::Entry> {
        // Both success and failure are terminal; no remote request is retried.
        let session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| failure(p::FailureCode::SessionClosed))?;
        session
            .client
            .put_file(&session.path, session.condition, session.bytes)
    }
    fn abort(&self) -> Result<()> {
        self.session
            .borrow_mut()
            .take()
            .ok_or_else(|| failure(p::FailureCode::SessionClosed))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> Result<WebdavStorage> {
        let config =
            p::JsonObject::from([("endpoint".into(), "https://webdav.invalid/root/".into())]);
        Ok(WebdavStorage {
            client: client::Client::parse(&config)?,
        })
    }

    #[test]
    fn driver_and_storage_do_not_advertise_partial_remove_support() -> Result<()> {
        let description = WebdavDriver::describe();
        let info = storage()?.info();
        assert_eq!(description.capabilities, info.capabilities);
        assert!(
            !description
                .capabilities
                .iter()
                .any(|capability| *capability == p::Capability::Remove as i32)
        );
        for capability in [
            p::Capability::Hierarchical as i32,
            p::Capability::EmptyDirs as i32,
            p::Capability::CreateDir as i32,
        ] {
            assert!(
                description
                    .capabilities
                    .iter()
                    .any(|actual| *actual == capability)
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
            let error = storage
                .call(p::StorageOp::Remove(p::Path { path: path.into() }), vec![])
                .err()
                .ok_or_else(|| failure(p::FailureCode::InvalidData))?;
            assert_eq!(
                error.code(),
                p::FailureCode::UnsupportedOperation,
                "path: {path}"
            );
        }
        Ok(())
    }
}
