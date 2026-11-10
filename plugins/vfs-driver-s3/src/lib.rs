#![cfg(target_arch = "wasm32")]
//! S3-compatible storage plugin using path-style requests and AWS SigV4.
//! Endpoint, bucket and signing region are supplied by instance configuration.
//! The portable component uses WASIp2 HTTP and the standard library clock. Directories are implicit prefixes; empty-directory creation,
//! rename, permissions and xattrs are deliberately unsupported. File/prefix
//! collisions are rejected rather than silently selecting one interpretation.
use ah_vfs_storage::{BackendFailure, ContractViolation, EntryType, Failure, Operation, Rejection};
mod s3;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use vfs_plugin_protocol::{self as p, Value};

vfs_plugin_guest::export_plugin!(S3Driver, "../../crates/vfs-plugin-protocol/wit");
use vfs_plugin_guest::api::{self, Guest, GuestReader, GuestStorage, GuestWriter};
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
fn caps() -> Vec<i32> {
    use ah_vfs_storage::{Capabilities, DirectorySupport, ReadSupport, WriteModes, WriteSupport};
    p::encode_capabilities(
        Capabilities::default()
            .with_stat(true)
            .with_list(true)
            .with_read(ReadSupport::Range)
            .with_write(WriteSupport::staged(WriteModes::Both, true))
            .with_directories(DirectorySupport::prefix(false, false))
            .with_remove(true),
    )
}
struct S3Driver;
impl Guest for S3Driver {
    fn describe() -> p::Description {
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
    fn info(&self) -> p::StorageInfo {
        p::StorageInfo {
            chunk_size: 64 * 1024,
        }
    }
    fn call(&self, request: p::StorageOp, data: Vec<u8>) -> Result<p::Reply> {
        let client = self.client.for_operation(request.storage_operation());
        let context = client.context.clone();
        if !data.is_empty() {
            return Err(context.failure(Failure::Rejected(Rejection::Conflict)));
        }
        let value = match request {
            p::StorageOp::Stat(request) => {
                let entry = api::direct_stat(request, |path| client.stat(path.as_str()))?;
                Value::Entry(encode_entry(&context, &entry)?)
            }
            p::StorageOp::List(p::List { path, after, limit }) => {
                let (entries, next_after) = client.list(&path, after.as_deref(), limit)?;
                Value::Page(p::Page {
                    entries: entries
                        .iter()
                        .map(|entry| encode_entry(&context, entry))
                        .collect::<Result<_>>()?,
                    next_after,
                })
            }
            p::StorageOp::Remove(p::Path { path }) => {
                let entry = client.stat(&path)?;
                if path.is_empty() {
                    return Err(context.failure(Failure::Rejected(Rejection::RootProtected)));
                }
                if entry.entry_type() == EntryType::Directory {
                    return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
                }
                client.request("DELETE", &path, vec![], vec![], vec![])?;
                Value::Unit(p::Empty {})
            }
            _ => return Err(context.failure(Failure::Rejected(Rejection::Unsupported))),
        };
        Ok(p::Reply::new(value))
    }
    fn read(&self, path: String, offset: u64, length: Option<u64>) -> Result<api::Reader> {
        let client = self.client.for_operation(Operation::Read);
        let context = client.context.clone();
        let entry = client.stat(&path)?;
        let EntryType::File { size, .. } = entry.entry_type() else {
            return Err(context.failure(Failure::Rejected(Rejection::IsDirectory)));
        };
        let size =
            size.ok_or_else(|| context.failure(Failure::Backend(BackendFailure::CorruptData)))?;
        Ok(api::Reader::new(S3Reader {
            client: client.clone(),
            path,
            offset: Cell::new(offset),
            remaining: Cell::new(length.unwrap_or(u64::MAX).min(size.saturating_sub(offset))),
        }))
    }
    fn write(&self, path: String, replace: bool, _atomic: bool) -> Result<api::Writer> {
        let client = self.client.for_operation(Operation::Write);
        path_check_target(&client, &path, replace)?;
        let upload = client.start_upload(&path)?;
        Ok(api::Writer::new(S3Writer {
            session: RefCell::new(Some(Upload {
                client: client.clone(),
                path,
                replace,
                upload,
                buffer: vec![],
                parts: vec![],
                size: 0,
            })),
        }))
    }
}
fn path_check_target(client: &s3::Client, name: &str, replace: bool) -> Result<()> {
    let context = client.context.clone();
    path(&context, name)?;
    if name.is_empty() {
        return Err(context.failure(Failure::Rejected(Rejection::RootProtected)));
    }
    for (index, _) in name.match_indices('/') {
        if client.head(&name[..index])?.is_some() {
            return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
        }
    }
    match client.stat(name) {
        Ok(entry) => {
            if !replace {
                return Err(context.failure(Failure::Rejected(Rejection::AlreadyExists)));
            }
            if !matches!(entry.entry_type(), EntryType::File { .. }) {
                return Err(context.failure(Failure::Rejected(Rejection::IsDirectory)));
            }
        }
        Err(e)
            if e.storage_error()
                .is_some_and(|error| error.failure() == Failure::Rejected(Rejection::NotFound)) =>
        {
            ()
        }
        Err(e) => return Err(e),
    }
    Ok(())
}
struct S3Reader {
    client: s3::Client,
    path: String,
    offset: Cell<u64>,
    remaining: Cell<u64>,
}
impl GuestReader for S3Reader {
    fn next(&self) -> Result<Option<Vec<u8>>> {
        let context = api::ErrorContext::storage(Operation::Read);
        let remaining = self.remaining.get();
        if remaining == 0 {
            return Ok(None);
        }
        let length = remaining.min(64 * 1024);
        let offset = self.offset.get();
        // Mark the session exhausted before I/O: failure must not skip bytes on retry.
        self.remaining.set(0);
        let response = self.client.request(
            "GET",
            &self.path,
            vec![],
            vec![(
                "range".into(),
                format!("bytes={offset}-{}", offset + length - 1),
            )],
            vec![],
        )?;
        if response.status().as_u16() != 206
            || response.body().is_empty()
            || response.body().len() as u64 > length
        {
            return Err(context.failure(Failure::Backend(BackendFailure::CorruptData)));
        }
        self.offset.set(offset + response.body().len() as u64);
        self.remaining.set(remaining - response.body().len() as u64);
        Ok(Some(response.into_body()))
    }
}
const PART_SIZE: usize = 5 * 1024 * 1024;
struct Upload {
    client: s3::Client,
    path: String,
    replace: bool,
    upload: String,
    buffer: Vec<u8>,
    parts: Vec<String>,
    size: u64,
}
impl Upload {
    fn upload_part(&mut self, bytes: Vec<u8>) -> Result<()> {
        let context = api::ErrorContext::storage(Operation::Write);
        if self.parts.len() >= 10_000 {
            return Err(context.failure(Failure::Backend(BackendFailure::CapacityExceeded)));
        }
        let etag =
            self.client
                .upload_part(&self.path, &self.upload, self.parts.len() + 1, bytes)?;
        self.parts.push(etag);
        Ok(())
    }
    fn abort(&self) -> Result<()> {
        self.client.abort_upload(&self.path, &self.upload)
    }
}
struct S3Writer {
    session: RefCell<Option<Upload>>,
}
impl GuestWriter for S3Writer {
    fn write(&self, data: Vec<u8>) -> Result<()> {
        let context = api::ErrorContext::storage(Operation::Write);
        if data.len() > p::MAX_DATA_BYTES {
            return Err(context.failure(Failure::Rejected(Rejection::LimitExceeded)));
        }
        let mut pending = self.session.borrow_mut();
        let s = pending
            .as_mut()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        s.size = s
            .size
            .checked_add(data.len() as u64)
            .ok_or_else(|| context.failure(Failure::Rejected(Rejection::LimitExceeded)))?;
        s.buffer.extend_from_slice(&data);
        while s.buffer.len() >= PART_SIZE {
            let part = s.buffer.drain(..PART_SIZE).collect();
            s.upload_part(part)?;
        }
        Ok(())
    }
    fn commit(&self) -> Result<p::Entry> {
        let context = api::ErrorContext::storage(Operation::Write);
        let mut s = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| context.failure(Failure::Backend(BackendFailure::Closed)))?;
        let outcome = (|| {
            path_check_target(&s.client, &s.path, s.replace)?;
            if s.size == 0 {
                s.abort()?;
                let headers = if s.replace {
                    vec![]
                } else {
                    vec![("if-none-match".into(), "*".into())]
                };
                s.client.request("PUT", &s.path, vec![], headers, vec![])?;
            } else {
                if !s.buffer.is_empty() {
                    let bytes = std::mem::take(&mut s.buffer);
                    s.upload_part(bytes)?;
                }
                s.client
                    .complete_upload(&s.path, &s.upload, &s.parts, s.replace)?;
            }
            s.client
                .stat(&s.path)
                .and_then(|entry| encode_entry(&s.client.context, &entry))
        })();
        if outcome.is_err() {
            let _ = s.abort();
        }
        outcome
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
impl Drop for S3Writer {
    fn drop(&mut self) {
        if let Some(s) = self.session.get_mut().take() {
            let _ = s.abort();
        }
    }
}

fn encode_entry(context: &api::ErrorContext, entry: &ah_vfs_storage::Entry) -> Result<p::Entry> {
    p::encode_entry(entry)
        .map_err(|_| context.failure(Failure::Contract(ContractViolation::InvalidResponse)))
}
