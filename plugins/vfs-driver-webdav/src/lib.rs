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

use std::num::NonZeroUsize;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use vfs_plugin_protocol as p;

vfs_plugin_guest::export_plugin!(WebdavDriver, "../../crates/vfs-plugin-protocol/wit");
use vfs_plugin_guest::api::{self, Description, Guest, GuestReader, GuestStorage, GuestWriter};

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
fn capabilities() -> ah_vfs_storage::Capabilities {
    use ah_vfs_storage::{Capabilities, DirectorySupport, ReadSupport, WriteModes, WriteSupport};
    Capabilities::default()
        .with_stat(true)
        .with_list(true)
        .with_read(ReadSupport::Range)
        .with_write(WriteSupport::staged(WriteModes::Both, false))
        .with_directories(DirectorySupport::hierarchical(true, true))
}

struct WebdavDriver;
impl Guest for WebdavDriver {
    fn describe() -> Description {
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
        Description {
            required_features: vec![],
            required_host_capabilities: vec!["http".into()],
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
        Ok(api::Storage::new(WebdavStorage {
            client,
            directory_generation: *ah_id::UuidV7::new().as_ref().as_bytes(),
        }))
    }
}

struct WebdavStorage {
    client: client::Client,
    directory_generation: [u8; 16],
}
impl GuestStorage for WebdavStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(CHUNK_SIZE as usize).unwrap_or(NonZeroUsize::MIN)
    }

    fn begin_lookup(&self, operation: Operation) -> Result<api::Lookup> {
        Ok(api::Lookup::new(WebdavLookup {
            client: self.client.for_operation(operation),
            generation: self.directory_generation,
            bindings: api::DirectBindings::new(operation),
        }))
    }

    fn resume_directory(
        &self,
        cursor: &ah_vfs_storage::DirectoryCursor,
    ) -> Result<ah_vfs_storage::DirectorySelection> {
        self.client
            .for_operation(Operation::List)
            .resume_directory(cursor, &self.directory_generation)
    }

    fn list_page(
        &self,
        bound: &ah_vfs_storage::BoundListRequest,
    ) -> Result<ah_vfs_storage::DirectoryPage> {
        self.client
            .for_operation(Operation::List)
            .list_page(bound, &self.directory_generation)
    }
}

struct WebdavLookup {
    client: client::Client,
    generation: [u8; 16],
    bindings: api::DirectBindings<client::Metadata>,
}

impl WebdavLookup {
    fn metadata(
        &self,
        path: &ah_vfs_storage::StoragePath,
    ) -> Result<(ah_vfs_storage::Entry, client::Metadata)> {
        let metadata = self.client.stat(path.as_str())?;
        Ok((metadata.entry.clone(), metadata))
    }
}

impl api::GuestLookup for WebdavLookup {
    fn root(&mut self) -> Result<(u64, ah_vfs_storage::Entry)> {
        let path = ah_vfs_storage::StoragePath::root();
        let metadata = self.metadata(&path);
        self.bindings.observe(path, metadata)
    }

    fn observe(
        &mut self,
        parent: u64,
        name: &ah_vfs_storage::EntryName,
    ) -> Result<(u64, ah_vfs_storage::Entry)> {
        let path = self.bindings.child_path(parent, name)?;
        let metadata = self.metadata(&path);
        self.bindings.observe(path, metadata)
    }

    fn read_link(&self, id: u64) -> Result<ah_vfs_storage::LinkTarget> {
        self.bindings.get(id)?;
        Err(self
            .client
            .context
            .failure(Failure::Rejected(Rejection::NotSymlink)))
    }

    fn verify(&self) -> Result<()> {
        self.bindings.verify(|path| self.metadata(path))
    }

    fn execute_stat(
        &self,
        id: u64,
        request: ah_vfs_storage::StatRequest,
    ) -> Result<ah_vfs_storage::Entry> {
        let (entry, _) = self.bindings.get(id)?;
        if entry.path() != request.path() {
            return Err(self.client.context.failure(Failure::InvalidBinding));
        }
        self.verify()?;
        Ok(entry.clone())
    }

    fn execute_create_directory(
        &mut self,
        slot: api::EntrySlot<'_>,
        request: ah_vfs_storage::CreateDirectoryRequest,
    ) -> Result<(u64, ah_vfs_storage::Entry)> {
        let client = self.client.for_operation(Operation::CreateDirectory);
        let path = request.path();
        let (parent, target) = self.bindings.slot(slot, path)?;
        request
            .check_target(target.map(|(entry, _)| entry.entry_type()))
            .map_err(|reason| client.context.failure(Failure::Rejected(reason)))?;
        if !client.make_collection(path, parent)? {
            self.bindings.allow_reobserve(path);
            return Err(client
                .context
                .failure(Failure::Rejected(Rejection::AlreadyExists)));
        }
        let metadata = client.stat(path.as_str())?;
        if metadata.entry.entry_type() != EntryType::Directory {
            return Err(client
                .context
                .failure(Failure::Backend(BackendFailure::Other)));
        }
        self.bindings
            .record_creation(path, (metadata.entry.clone(), metadata))
            .map_err(|error| {
                error
                    .storage_error()
                    .map(|error| client.context.failure(error.failure()))
                    .unwrap_or(error)
            })
    }

    fn select_directory(
        &self,
        id: u64,
        request: ah_vfs_storage::ListRequest,
    ) -> Result<ah_vfs_storage::DirectorySelection> {
        let (entry, metadata) = self.bindings.get(id)?;
        if entry.path() != request.directory() || request.after().is_some() {
            return Err(self.client.context.failure(Failure::InvalidBinding));
        }
        self.verify()?;
        self.client
            .select_observed_directory(metadata, &self.generation)
    }

    fn execute_read(
        &self,
        slot: api::EntrySlot<'_>,
        request: ah_vfs_storage::ReadRequest,
    ) -> Result<api::Reader> {
        let client = self.client.clone();
        let context = client.context.clone();
        let (_, selected) = self.bindings.slot(slot, request.path())?;
        let (entry, metadata) = selected.ok_or_else(|| context.failure(Failure::InvalidBinding))?;
        request
            .check_target(entry.entry_type())
            .map_err(|reason| context.failure(Failure::Rejected(reason)))?;
        let etag = metadata.strong_etag();
        let EntryType::File { size, .. } = entry.entry_type() else {
            return Err(context.failure(Failure::Rejected(Rejection::NotRegularFile)));
        };
        let size =
            size.ok_or_else(|| context.failure(Failure::Rejected(Rejection::Unsupported)))?;
        let path = entry.path().as_str().to_owned();
        let offset = request.range().offset();
        let remaining = request
            .range()
            .length()
            .unwrap_or(u64::MAX)
            .min(size.saturating_sub(offset));
        let cached = if etag.is_none() && remaining != 0 {
            // Bind one response instead of joining unversioned representations.
            // The host applies its HTTP response limit to the whole range.
            let (data, _) = client.read_range(&path, offset, remaining, size, None)?;
            Some((offset, data))
        } else {
            None
        };
        Ok(api::Reader::new(WebdavReader {
            session: RefCell::new(Some(ReadSession {
                client,
                path,
                offset,
                remaining,
                size,
                etag,
                cached,
            })),
            eof: Cell::new(false),
        }))
    }
    fn execute_write(
        &self,
        slot: api::EntrySlot<'_>,
        request: ah_vfs_storage::WriteRequest,
    ) -> Result<api::Writer> {
        let client = self.client.clone();
        let context = &client.context;
        let (parent, target) = self.bindings.slot(slot, request.path())?;
        request
            .check_target(target.map(|(entry, _)| entry.entry_type()))
            .map_err(|reason| context.failure(Failure::Rejected(reason)))?;
        let condition = client.write_condition(parent, target.map(|(_, metadata)| metadata))?;
        let path = request.path().as_str().to_owned();
        Ok(api::Writer::new(WebdavWriter {
            session: RefCell::new(Some(WriteSession {
                client,
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
    cached: Option<(u64, Vec<u8>)>,
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
        let data = if let Some((base, bytes)) = &session.cached {
            let start = usize::try_from(session.offset - base)
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
            session.cached = full.map(|bytes| (0, bytes));
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
    fn commit(&self) -> Result<ah_vfs_storage::Entry> {
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
            directory_generation: [1; 16],
        })
    }

    #[test]
    fn driver_declaration_excludes_partial_remove_support() -> Result<()> {
        let description = WebdavDriver::describe();
        let caps = description.capabilities;
        assert_eq!(caps, capabilities());
        assert_eq!(storage()?.chunk_size().get(), CHUNK_SIZE as usize);
        assert!(!caps.remove());
        assert_eq!(
            caps.directories(),
            ah_vfs_storage::DirectorySupport::hierarchical(true, true)
        );
        Ok(())
    }

    #[test]
    fn remove_is_rejected_without_http_for_every_entry_kind() -> Result<()> {
        let storage = storage()?;
        // Native WIT HTTP imports panic if called, so this also verifies that
        // unsupported removal does not inspect or mutate remote resources.
        for path in ["file", "empty-dir", "nonempty-dir", "missing"] {
            use ah_vfs_storage::{EntryName, RemoveKind, RemoveRequest, StoragePath};
            use api::GuestLookup;
            let context = api::ErrorContext::storage(Operation::Remove);
            let address = StoragePath::parse(path).map_err(|_| {
                context.failure(Failure::Contract(ContractViolation::InvalidResponse))
            })?;
            let invalid = || context.failure(Failure::Contract(ContractViolation::InvalidResponse));
            let name = EntryName::parse(path).map_err(|_| invalid())?;
            let request =
                RemoveRequest::new(address, RemoveKind::NonDirectory).map_err(|_| invalid())?;
            let lookup = WebdavLookup {
                client: storage.client.for_operation(Operation::Remove),
                generation: storage.directory_generation,
                bindings: api::DirectBindings::new(Operation::Remove),
            };
            let slot = api::EntrySlot {
                parent: 1,
                name: &name,
                entry: None,
            };
            let error = lookup.execute_remove(slot, request).err().ok_or_else(|| {
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
