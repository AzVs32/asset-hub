//! S3-compatible storage plugin using path-style requests and AWS SigV4.
//! Endpoint, bucket and signing region are supplied by instance configuration.
//! The portable component imports only bounded HTTP
//! and a wall clock. Directories are implicit prefixes; empty-directory creation,
//! rename, permissions and xattrs are deliberately unsupported. File/prefix
//! collisions are rejected rather than silently selecting one interpretation.
#![cfg(target_arch = "wasm32")]
mod s3;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use vfs_plugin_protocol::{self as p, Value};

wit_bindgen::generate!({path:"../../crates/vfs-plugin-protocol/wit",world:"plugin"});
use assethub::vfs::{clock, http};
use exports::assethub::vfs::driver::{self as api, Guest, GuestReader, GuestStorage, GuestWriter};
type Result<T> = std::result::Result<T, api::Failure>;
fn failure(code: &str) -> api::Failure {
    api::Failure {
        code: code.into(),
        field: None,
        config_kind: None,
    }
}
fn invalid(field: &str, kind: &str) -> api::Failure {
    api::Failure {
        code: "invalid_instance_config".into(),
        field: Some(field.into()),
        config_kind: Some(kind.into()),
    }
}
fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| failure("invalid_data"))
}
fn object(bytes: &[u8]) -> Result<p::Object> {
    if bytes.len() > p::MAX_CONTROL_BYTES {
        return Err(failure("too_large"));
    }
    serde_json::from_slice(bytes).map_err(|_| failure("invalid_request"))
}
fn path(path: &str) -> Result<()> {
    if path.len() > 4096
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
        || (!path.is_empty() && path.split('/').any(|s| matches!(s, "" | "." | "..")))
    {
        return Err(failure("invalid_path"));
    }
    Ok(())
}
fn caps() -> Vec<String> {
    [
        "stat",
        "list",
        "range_read",
        "write_new",
        "write_replace",
        "atomic_publish",
        "prefix",
        "remove",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
struct S3Driver;
impl Guest for S3Driver {
    type Storage = S3Storage;
    type Reader = S3Reader;
    type Writer = S3Writer;
    fn describe() -> Vec<u8> {
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
        encode(&p::Description {
            major: p::PROTOCOL_MAJOR,
            minor: p::PROTOCOL_MINOR,
            required_features: vec![],
            features: p::PROTOCOL_FEATURES.iter().map(|feature| (*feature).into()).collect(),
            required_host_capabilities: vec![p::HOST_HTTP.into(), p::HOST_CLOCK.into()],
            instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
            generation: String::new(),
            key: "s3".into(),
            name: "S3 object storage".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            child_mounts: true,
            capabilities: caps(),
            ownership: p::Ownership::Host,
            settings_schema: None,
            instance_schema: Some(schema),
            config_version: 1,
        })
        .unwrap_or_default()
    }
    fn initialize(settings: Vec<u8>) -> Result<()> {
        if object(&settings)?.is_empty() {
            Ok(())
        } else {
            Err(failure("unsupported_operation"))
        }
    }
    fn prepare(config: Vec<u8>) -> Result<Vec<u8>> {
        let fields = object(&config)?;
        let client = s3::Client::parse(&fields)?;
        encode(&Value::Prepared {
            config: fields,
            details: BTreeMap::from([
                ("endpoint".into(), client.endpoint.into()),
                ("bucket".into(), client.bucket.into()),
            ]),
        })
    }
    fn migrate(from_version: u32, config: Vec<u8>) -> Result<Vec<u8>> {
        if from_version != 1 {
            return Err(failure("unsupported_operation"));
        }
        let fields = object(&config)?;
        s3::Client::parse(&fields)?;
        encode(&fields)
    }
    fn test_connection(config: Vec<u8>) -> Result<()> {
        s3::Client::parse(&object(&config)?)?.check_bucket()
    }
    fn instances(_control: Vec<u8>) -> Result<Vec<u8>> {
        Err(failure("unsupported_operation"))
    }
    fn open(_id: String, config: Option<Vec<u8>>) -> Result<api::Storage> {
        let config = config.ok_or_else(|| failure("invalid_request"))?;
        let client = s3::Client::parse(&object(&config)?)?;
        client.check_bucket()?;
        Ok(api::Storage::new(S3Storage { client }))
    }
}
struct S3Storage {
    client: s3::Client,
}
impl GuestStorage for S3Storage {
    fn info(&self) -> Vec<u8> {
        encode(&p::StorageInfo {
            capabilities: caps(),
            chunk_size: 64 * 1024,
        })
        .unwrap_or_default()
    }
    fn call(&self, control: Vec<u8>, data: Vec<u8>) -> Result<api::Reply> {
        if control.len() > p::MAX_CONTROL_BYTES || !data.is_empty() {
            return Err(failure("invalid_request"));
        }
        let request: p::StorageOp =
            serde_json::from_slice(&control).map_err(|_| failure("invalid_request"))?;
        let value = match request {
            p::StorageOp::Stat { path } => Value::Entry {
                entry: self.client.stat(&path)?,
            },
            p::StorageOp::List { path, after, limit } => {
                let (entries, next_after) = self.client.list(&path, after.as_deref(), limit)?;
                Value::Page {
                    entries,
                    next_after,
                }
            }
            p::StorageOp::Remove { path } => {
                let entry = self.client.stat(&path)?;
                if path.is_empty() {
                    return Err(failure("root_protected"));
                }
                if entry.kind == p::EntryKind::Dir {
                    return Err(failure("unsupported_operation"));
                }
                self.client
                    .request("DELETE", &path, vec![], vec![], vec![])?;
                Value::Unit
            }
            _ => return Err(failure("unsupported_operation")),
        };
        Ok(api::Reply {
            control: encode(&value)?,
            data: vec![],
        })
    }
    fn read(&self, path: String, offset: u64, length: Option<u64>) -> Result<api::Reader> {
        let entry = self.client.stat(&path)?;
        if entry.kind != p::EntryKind::File {
            return Err(failure("is_dir"));
        }
        let size = entry.size.ok_or_else(|| failure("invalid_data"))?;
        Ok(api::Reader::new(S3Reader {
            client: self.client.clone(),
            path,
            offset: Cell::new(offset),
            remaining: Cell::new(length.unwrap_or(u64::MAX).min(size.saturating_sub(offset))),
        }))
    }
    fn write(&self, path: String, replace: bool, _atomic: bool) -> Result<api::Writer> {
        path_check_target(&self.client, &path, replace)?;
        let upload = self.client.start_upload(&path)?;
        Ok(api::Writer::new(S3Writer {
            session: RefCell::new(Some(Upload {
                client: self.client.clone(),
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
    path(name)?;
    if name.is_empty() {
        return Err(failure("root_protected"));
    }
    for (index, _) in name.match_indices('/') {
        if client.head(&name[..index])?.is_some() {
            return Err(failure("not_dir"));
        }
    }
    match client.stat(name) {
        Ok(entry) => {
            if !replace {
                return Err(failure("already_exists"));
            }
            if entry.kind != p::EntryKind::File {
                return Err(failure("is_dir"));
            }
        }
        Err(e) if e.code == "not_found" => (),
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
        if response.status != 206 || response.body.is_empty() || response.body.len() as u64 > length
        {
            return Err(failure("invalid_data"));
        }
        self.offset.set(offset + response.body.len() as u64);
        self.remaining.set(remaining - response.body.len() as u64);
        Ok(Some(response.body))
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
        if self.parts.len() >= 10_000 {
            return Err(failure("capacity_exceeded"));
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
        if data.len() > p::MAX_DATA_BYTES {
            return Err(failure("too_large"));
        }
        let mut pending = self.session.borrow_mut();
        let s = pending.as_mut().ok_or_else(|| failure("session_closed"))?;
        s.size = s
            .size
            .checked_add(data.len() as u64)
            .ok_or_else(|| failure("too_large"))?;
        s.buffer.extend_from_slice(&data);
        while s.buffer.len() >= PART_SIZE {
            let part = s.buffer.drain(..PART_SIZE).collect();
            s.upload_part(part)?;
        }
        Ok(())
    }
    fn commit(&self) -> Result<Vec<u8>> {
        let mut s = self
            .session
            .borrow_mut()
            .take()
            .ok_or_else(|| failure("session_closed"))?;
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
                .map_err(|_| failure("outcome_unknown"))
        })();
        if outcome.is_err() {
            let _ = s.abort();
        }
        encode(&outcome?)
    }
    fn abort(&self) -> Result<()> {
        self.session
            .borrow_mut()
            .take()
            .ok_or_else(|| failure("session_closed"))?
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

#[cfg(target_arch = "wasm32")]
export!(S3Driver);
