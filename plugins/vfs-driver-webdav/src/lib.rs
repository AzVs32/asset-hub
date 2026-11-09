//! Portable WebDAV driver using standard WASIp2 HTTP.
//!
//! Instance records and credential encryption belong to the host. Authentication
//! is anonymous, Basic or Bearer. The endpoint is an existing collection URL.
//! Staged PUT writes are bounded to max_write_bytes (at most 8 MiB), matching the
//! bounded whole-file staging policy. Larger reads require server byte-range support.
//! Atomic publication/rename, POSIX permissions and xattrs are not advertised.
//! Removal is not advertised or implemented: RFC 4918 collection DELETE is
//! recursive and cannot guarantee the VFS empty-directory removal contract.
#![cfg(any(target_arch = "wasm32", test))]

use ah_vfs_storage::{BackendFailure, ContractViolation, EntryType, Failure, Operation, Rejection};
mod client;
mod multistatus;

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use vfs_plugin_protocol::{self as p, Value};

vfs_plugin_guest::export_plugin!(WebdavDriver, "../../crates/vfs-plugin-protocol/wit");
use vfs_plugin_guest::api::{self, Guest, GuestReader, GuestStorage, GuestWriter};

type Result<T> = std::result::Result<T, api::Failure>;
const CHUNK_SIZE: u32 = 64 * 1024;

fn invalid(field: &str, kind: p::ConfigErrorKind) -> api::Failure {
    p::Failure {
        code: p::FailureCode::InvalidInstanceConfig as i32,
        field: Some(field.into()),
        config_kind: Some(kind as i32),
        storage: None,
    }
    .into()
}
fn path(context: &api::ErrorContext, value: &str) -> Result<()> {
    ah_vfs_storage::StoragePath::parse(value)
        .map(|_| ())
        .map_err(|_| context.failure(Failure::Rejected(Rejection::UnsupportedName)))
}
fn capabilities() -> Vec<i32> {
    use ah_vfs_storage::{Capabilities, DirectorySupport, ReadSupport, WriteModes, WriteSupport};
    p::encode_capabilities(
        Capabilities::default()
            .with_stat(true)
            .with_list(true)
            .with_read(ReadSupport::Range)
            .with_write(WriteSupport::staged(WriteModes::Both, false))
            .with_directories(DirectorySupport::hierarchical(true, true)),
    )
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
            required_features: vec![],
            features: p::PROTOCOL_FEATURES
                .iter()
                .map(|feature| (*feature).into())
                .collect(),
            required_host_capabilities: vec!["http".into()],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: String::new(),
            key: "webdav".into(),
            name: "WebDAV".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            child_mounts: true,
            capabilities: capabilities(),

            settings_schema: None,
            instance_schema: instance_schema.map(Into::into),
            config_version: 1,
        }
    }
    fn initialize(settings: p::Object) -> Result<()> {
        let context = api::ErrorContext::lifecycle();
        if settings.fields.is_empty() {
            Ok(())
        } else {
            Err(context.failure(Failure::Rejected(Rejection::Unsupported)))
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
        let context = api::ErrorContext::lifecycle();
        if from_version != 1 {
            return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
        }
        let fields = p::json_object(config)?;
        client::Client::parse(&fields)?;
        Ok(fields.into())
    }
    fn test_connection(config: p::Object) -> Result<()> {
        client::Client::parse(&p::json_object(config)?)?.check_root()
    }
    fn open(_id: String, config: p::Object) -> Result<api::Storage> {
        let config = p::json_object(config)?;
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
            chunk_size: CHUNK_SIZE,
        }
    }
    fn call(&self, request: p::StorageOp, data: Vec<u8>) -> Result<p::Reply> {
        let client = self.client.for_operation(request.storage_operation());
        let context = client.context.clone();
        if !data.is_empty() {
            return Err(context.failure(Failure::Rejected(Rejection::Conflict)));
        }
        let value = match request {
            p::StorageOp::Stat(p::Path { path }) => {
                Value::Entry(encode_entry(&context, &client.stat(&path)?.entry)?)
            }
            p::StorageOp::List(p::List { path, after, limit }) => {
                let (entries, next_after) = client.list(&path, after.as_deref(), limit)?;
                Value::Page(p::Page {
                    entries,
                    next_after,
                })
            }
            p::StorageOp::CreateDir(p::CreateDir { path, recursive }) => Value::Entry(
                encode_entry(&context, &client.create_dir(&path, recursive)?)?,
            ),
            _ => return Err(context.failure(Failure::Rejected(Rejection::Unsupported))),
        };
        Ok(p::Reply::new(value))
    }
    fn read(&self, path: String, offset: u64, length: Option<u64>) -> Result<api::Reader> {
        let client = self.client.for_operation(Operation::Read);
        let context = client.context.clone();
        let metadata = client.stat(&path)?;
        if metadata.entry.entry_type() == EntryType::Directory {
            return Err(context.failure(Failure::Rejected(Rejection::IsDirectory)));
        }
        let EntryType::File { size, .. } = metadata.entry.entry_type() else {
            return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
        };
        let size =
            size.ok_or_else(|| context.failure(Failure::Rejected(Rejection::Unsupported)))?;
        Ok(api::Reader::new(WebdavReader {
            session: RefCell::new(Some(ReadSession {
                client: client.clone(),
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
        let client = self.client.for_operation(Operation::Write);
        let context = client.context.clone();
        if atomic {
            return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
        }
        let condition = client.write_condition(&path, replace)?;
        Ok(api::Writer::new(WebdavWriter {
            session: RefCell::new(Some(WriteSession {
                client: client.clone(),
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
        let context = api::ErrorContext::storage(Operation::Read);
        if self.eof.get() {
            return Ok(None);
        }
        // Take before I/O so errors cannot silently resume at an uncertain offset.
        let mut session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        if session.remaining == 0 {
            self.eof.set(true);
            return Ok(None);
        }
        let length = session.remaining.min(u64::from(CHUNK_SIZE));
        let data = if let Some(bytes) = &session.cached {
            let start = usize::try_from(session.offset)
                .map_err(|_| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
            let end = start
                .checked_add(length as usize)
                .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
            bytes
                .get(start..end)
                .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?
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
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        session.offset = session
            .offset
            .checked_add(data.len() as u64)
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
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
        let context = api::ErrorContext::storage(Operation::Write);
        let mut session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        let length = session
            .bytes
            .len()
            .checked_add(data.len())
            .ok_or_else(|| context.failure(Failure::Rejected(Rejection::LimitExceeded)))?;
        if length > session.client.max_write_bytes {
            return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
        }
        session
            .bytes
            .try_reserve_exact(data.len())
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CapacityExceeded)))?;
        session.bytes.extend_from_slice(&data);
        *self.session.borrow_mut() = Some(session);
        Ok(())
    }
    fn commit(&self) -> Result<p::Entry> {
        let context = api::ErrorContext::storage(Operation::Write);
        // Both success and failure are terminal; no remote request is retried.
        let session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        session
            .client
            .put_file(&session.path, session.condition, session.bytes)
            .and_then(|entry| encode_entry(&session.client.context, &entry))
    }
    fn abort(&self) -> Result<()> {
        let context = api::ErrorContext::storage(Operation::Write);
        self.session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
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
    fn driver_declaration_excludes_partial_remove_support() -> Result<()> {
        let description = WebdavDriver::describe();
        let info = storage()?.info();
        assert_eq!(description.capabilities, capabilities());
        assert_eq!(info.chunk_size, CHUNK_SIZE);
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
                .ok_or_else(|| {
                    api::ErrorContext::storage(Operation::Remove)
                        .failure(Failure::Contract(ContractViolation::InvalidResponse))
                })?;
            assert_eq!(
                error.storage_error().map(|error| (
                    error.operation(),
                    error.failure(),
                    error.outcome()
                )),
                Some((
                    Operation::Remove,
                    Failure::Rejected(Rejection::Unsupported),
                    ah_vfs_storage::MutationOutcome::Unchanged
                )),
                "path: {path}"
            );
        }
        Ok(())
    }
}

fn encode_entry(context: &api::ErrorContext, entry: &ah_vfs_storage::Entry) -> Result<p::Entry> {
    p::encode_entry(entry)
        .map_err(|_| context.failure(Failure::Contract(ContractViolation::InvalidResponse)))
}
