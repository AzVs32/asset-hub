#![cfg(target_arch = "wasm32")]
//! S3-compatible storage plugin using path-style requests and AWS SigV4.
//! Endpoint, bucket and signing region are supplied by instance configuration.
//! The portable component uses WASIp2 HTTP and the standard library clock. Directories are implicit prefixes; empty-directory creation,
//! rename, permissions and xattrs are deliberately unsupported. File/prefix
//! collisions are rejected rather than silently selecting one interpretation.
use ah_vfs_storage::{BackendFailure, ContractViolation, EntryType, Failure, Operation, Rejection};
mod s3;
use std::num::NonZeroUsize;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use vfs_plugin_protocol as p;

vfs_plugin_guest::export_plugin!(S3Driver, "../../crates/vfs-plugin-protocol/wit");
use vfs_plugin_guest::api::{self, Description, Guest, GuestReader, GuestStorage, GuestWriter};
type Result<T> = std::result::Result<T, api::Failure>;
fn invalid(field: &str, kind: p::ConfigErrorKind) -> api::Failure {
    p::Failure {
        code: p::FailureCode::InvalidInstanceConfig as i32,
        field: Some(field.into()),
        config_kind: Some(kind as i32),
        storage: None,
    }
    .into()
}
fn path(context: &api::ErrorContext, path: &str) -> Result<()> {
    ah_vfs_storage::StoragePath::parse(path)
        .map(|_| ())
        .map_err(|_| context.failure(Failure::Rejected(Rejection::UnsupportedName)))
}
fn caps() -> ah_vfs_storage::Capabilities {
    use ah_vfs_storage::{Capabilities, DirectorySupport, ReadSupport, WriteModes, WriteSupport};
    Capabilities::default()
        .with_stat(true)
        .with_read(ReadSupport::Range)
        .with_write(WriteSupport::staged(WriteModes::Both, true))
        .with_directories(DirectorySupport::prefix(false, false))
        .with_remove(true)
}
struct S3Driver;
impl Guest for S3Driver {
    fn describe() -> Description {
        let mut properties = serde_json::Map::new();
        for key in ["endpoint", "bucket", "region", "access_key", "secret_key"] {
            let secret = matches!(key, "access_key" | "secret_key");
            properties.insert(
                key.into(),
                serde_json::json!({"type":"string","x-secret":secret,"x-immutable":!secret}),
            );
        }
        let schema = BTreeMap::from([
            ("type".into(), "object".into()),
            ("properties".into(), properties.into()),
            (
                "required".into(),
                serde_json::json!(["endpoint", "bucket", "region", "access_key", "secret_key"]),
            ),
            ("additionalProperties".into(), false.into()),
        ]);
        Description {
            required_features: vec![],
            required_host_capabilities: vec!["http".into()],
            key: "s3".into(),
            name: "S3 object storage".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            child_mounts: true,
            capabilities: caps(),

            settings_schema: None,
            instance_schema: Some(schema.into()),
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
        let fields = p::json_object(config)?;
        let client = s3::Client::parse(&fields)?;
        Ok(p::Prepared {
            config: Some(fields.into()),
            details: Some(
                BTreeMap::from([
                    ("endpoint".into(), client.endpoint.into()),
                    ("bucket".into(), client.bucket.into()),
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
        s3::Client::parse(&fields)?;
        Ok(fields.into())
    }
    fn test_connection(config: p::Object) -> Result<()> {
        s3::Client::parse(&p::json_object(config)?)?.check_bucket()
    }

    fn open(_id: String, config: p::Object) -> Result<api::Storage> {
        let config = p::json_object(config)?;
        let client = s3::Client::parse(&config)?;
        client.check_bucket()?;
        Ok(api::Storage::new(S3Storage { client }))
    }
}
struct S3Storage {
    client: s3::Client,
}
impl GuestStorage for S3Storage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(64 * 1024).unwrap_or(NonZeroUsize::MIN)
    }

    fn begin_lookup(&self, operation: Operation) -> Result<api::Lookup> {
        Ok(api::Lookup::new(S3Lookup {
            client: self.client.for_operation(operation),
            bindings: api::DirectBindings::new(operation),
        }))
    }
}

struct S3Lookup {
    client: s3::Client,
    bindings: api::DirectBindings<Option<String>>,
}

impl api::GuestLookup for S3Lookup {
    fn root(&mut self) -> Result<(u64, ah_vfs_storage::Entry)> {
        self.bindings.observe(
            ah_vfs_storage::StoragePath::root(),
            self.client.stat_object(""),
        )
    }

    fn observe(
        &mut self,
        parent: u64,
        name: &ah_vfs_storage::EntryName,
    ) -> Result<(u64, ah_vfs_storage::Entry)> {
        let path = self.bindings.child_path(parent, name)?;
        let value = self.client.stat_object(path.as_str());
        self.bindings.observe(path, value)
    }

    fn read_link(&self, id: u64) -> Result<ah_vfs_storage::LinkTarget> {
        self.bindings.get(id)?;
        Err(self
            .client
            .context
            .failure(Failure::Rejected(Rejection::NotSymlink)))
    }

    fn verify(&self) -> Result<()> {
        self.bindings
            .verify(|path| self.client.stat_object(path.as_str()))
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

    fn execute_remove(
        &self,
        slot: api::EntrySlot<'_>,
        request: ah_vfs_storage::RemoveRequest,
    ) -> Result<()> {
        let client = &self.client;
        let context = &client.context;
        let (_, selected) = self.bindings.slot(slot, request.path())?;
        let (entry, etag) = selected.ok_or_else(|| context.failure(Failure::InvalidBinding))?;
        request
            .check_target(entry.entry_type())
            .map_err(|reason| context.failure(Failure::Rejected(reason)))?;
        let etag = etag.clone();
        let etag =
            etag.ok_or_else(|| context.failure(Failure::Rejected(Rejection::Unsupported)))?;
        let reply = client.request(
            "DELETE",
            entry.path().as_str(),
            vec![],
            vec![("if-match".into(), etag)],
            vec![],
        )?;
        if reply.status().as_u16() != 204 || !reply.body().is_empty() {
            return Err(context.failure(Failure::Contract(
                ah_vfs_storage::ContractViolation::InvalidResponse,
            )));
        }
        Ok(())
    }
    fn execute_read(
        &self,
        slot: api::EntrySlot<'_>,
        request: ah_vfs_storage::ReadRequest,
    ) -> Result<api::Reader> {
        let client = self.client.clone();
        let context = client.context.clone();
        let (_, selected) = self.bindings.slot(slot, request.path())?;
        let (entry, etag) = selected.ok_or_else(|| context.failure(Failure::InvalidBinding))?;
        request
            .check_target(entry.entry_type())
            .map_err(|reason| context.failure(Failure::Rejected(reason)))?;
        let etag = etag.clone();
        let EntryType::File {
            size: Some(size), ..
        } = entry.entry_type()
        else {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        };
        let offset = request.range().offset();
        let remaining = request
            .range()
            .length()
            .unwrap_or(u64::MAX)
            .min(size.saturating_sub(offset));
        if remaining == 0 {
            self.verify()?;
        } else if etag.is_none() {
            return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
        }
        Ok(api::Reader::new(S3Reader {
            client,
            path: entry.path().as_str().into(),
            etag,
            size,
            position: Cell::new(Some((offset, remaining))),
        }))
    }
    fn execute_write(
        &self,
        slot: api::EntrySlot<'_>,
        request: ah_vfs_storage::WriteRequest,
    ) -> Result<api::Writer> {
        let client = self.client.clone();
        let context = client.context.clone();
        let path = request.path().clone();
        let (_, selected) = self.bindings.slot(slot, &path)?;
        request
            .check_target(selected.map(|(entry, _)| entry.entry_type()))
            .map_err(|reason| context.failure(Failure::Rejected(reason)))?;
        let destination = selected.map(|(_, etag)| etag.clone());
        // A prefix has no directory identity which S3 can conditionally bind.
        if !path.parent().is_some_and(|parent| parent.is_root()) {
            return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
        }
        let target = match destination {
            Some(etag) => Some(
                etag.ok_or_else(|| context.failure(Failure::Rejected(Rejection::Unsupported)))?,
            ),
            None => None,
        };
        Ok(api::Writer::new(S3Writer {
            session: RefCell::new(Some(Upload {
                client,
                path,
                target,
                upload: None,
                buffer: vec![],
                parts: vec![],
                size: 0,
            })),
        }))
    }
}
struct S3Reader {
    client: s3::Client,
    path: String,
    etag: Option<String>,
    size: u64,
    position: Cell<Option<(u64, u64)>>,
}
impl GuestReader for S3Reader {
    fn next(&self) -> Result<Option<Vec<u8>>> {
        let context = api::ErrorContext::storage(Operation::Read);
        // A failed request leaves the stream closed rather than skipping bytes.
        let (offset, remaining) = self
            .position
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        if remaining == 0 {
            self.position.set(Some((offset, 0)));
            return Ok(None);
        }
        let length = remaining.min(64 * 1024);
        let end = offset + length - 1;
        let etag = self
            .etag
            .as_ref()
            .ok_or_else(|| context.failure(Failure::Rejected(Rejection::Unsupported)))?;
        let response = self.client.request(
            "GET",
            &self.path,
            vec![],
            vec![
                ("range".into(), format!("bytes={offset}-{end}")),
                ("if-match".into(), etag.clone()),
            ],
            vec![],
        )?;
        if s3::header(&response, "etag").as_deref() != Some(etag.as_str()) {
            return Err(context.failure(Failure::Rejected(Rejection::Conflict)));
        }
        let range = format!("bytes {offset}-{end}/{}", self.size);
        if response.status().as_u16() != 206
            || s3::header(&response, "content-range").as_deref() != Some(range.as_str())
            || response.body().len() as u64 != length
        {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        self.position
            .set(Some((offset + length, remaining - length)));
        Ok(Some(response.into_body()))
    }
}
const PART_SIZE: usize = 5 * 1024 * 1024;
struct Upload {
    client: s3::Client,
    path: ah_vfs_storage::StoragePath,
    target: Option<String>,
    upload: Option<String>,
    buffer: Vec<u8>,
    parts: Vec<String>,
    size: u64,
}
impl Upload {
    fn upload_part(&mut self, bytes: Vec<u8>) -> Result<()> {
        let context = &self.client.context;
        if self.parts.len() >= 10_000 {
            return Err(context.failure(Failure::Backend(BackendFailure::CapacityExceeded)));
        }
        let path = self.path.as_str();
        let upload = match &self.upload {
            Some(upload) => upload,
            None => self.upload.insert(self.client.start_upload(path)?),
        };
        let etag = self
            .client
            .upload_part(path, upload, self.parts.len() + 1, bytes)?;
        self.parts.push(etag);
        Ok(())
    }

    fn abort(&mut self) -> Result<()> {
        if let Some(upload) = self.upload.take() {
            self.client.abort_upload(self.path.as_str(), &upload)?;
        }
        Ok(())
    }
}
impl Drop for Upload {
    fn drop(&mut self) {
        let _ = self.abort();
    }
}
struct S3Writer {
    session: RefCell<Option<Upload>>,
}
impl GuestWriter for S3Writer {
    fn write(&self, data: Vec<u8>) -> Result<()> {
        let context = api::ErrorContext::storage(Operation::Write);
        let mut session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        if data.len() > p::MAX_DATA_BYTES {
            return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
        }
        session.size = session
            .size
            .checked_add(data.len() as u64)
            .ok_or_else(|| context.failure(Failure::Rejected(Rejection::LimitExceeded)))?;
        session
            .buffer
            .try_reserve(data.len())
            .map_err(|_| context.failure(Failure::Backend(BackendFailure::CapacityExceeded)))?;
        session.buffer.extend_from_slice(&data);
        while session.buffer.len() >= PART_SIZE {
            let part = session.buffer.drain(..PART_SIZE).collect();
            session.upload_part(part)?;
        }
        *self.session.borrow_mut() = Some(session);
        Ok(())
    }
    fn commit(&self) -> Result<ah_vfs_storage::Entry> {
        let context = api::ErrorContext::storage(Operation::Write);
        let mut session = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        let entry = ah_vfs_storage::Entry::new(
            session.path.clone(),
            EntryType::file(Some(session.size)),
            None,
        )
        .map_err(|_| context.failure(Failure::Contract(ContractViolation::InvalidEntry)))?;
        if session.size == 0 {
            session.client.request(
                "PUT",
                session.path.as_str(),
                vec![],
                vec![s3::write_condition(session.target.as_deref())],
                vec![],
            )?;
        } else {
            if !session.buffer.is_empty() {
                let bytes = std::mem::take(&mut session.buffer);
                session.upload_part(bytes)?;
            }
            let upload = session
                .upload
                .as_ref()
                .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
            session.client.complete_upload(
                session.path.as_str(),
                upload,
                &session.parts,
                session.target.as_deref(),
            )?;
            session.upload = None;
        }
        Ok(entry)
    }
    fn abort(&self) -> Result<()> {
        let context = api::ErrorContext::storage(Operation::Write);
        self.session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?
            .abort()
    }
}
