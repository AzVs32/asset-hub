//! Deterministic backend shared by all three conformance implementations.
//! Fault paths and counters are fixture controls, never production VFS extensions.
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use vfs_plugin_protocol::{self as p, Entry, EntryKind, Object};

pub type Result<T> = std::result::Result<T, p::Failure>;
pub const CHUNK: u32 = 3;
pub const SEED: &[u8] = b"0123456789";
pub fn failure(code: &str) -> p::Failure {
    p::Failure::new(code)
}

pub fn capabilities(read_only: bool) -> Vec<String> {
    let mut caps = vec!["stat", "list", "range_read", "hierarchical"];
    if !read_only {
        caps.extend(["write_new", "write_replace", "atomic_publish", "remove"]);
    }
    caps.into_iter().map(str::to_owned).collect()
}

pub fn description() -> p::Description {
    p::Description {
        major: p::PROTOCOL_MAJOR,
        minor: p::PROTOCOL_MINOR,
        required_features: vec![p::INSTANCE_SCHEMA_PROFILE.into()],
        features: p::PROTOCOL_FEATURES.iter().map(|s| (*s).into()).collect(),
        required_host_capabilities: vec![],
        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
        generation: "conformance".into(),
        key: "conformance".into(),
        name: "ConformanceDriver".into(),
        version: "0.1.0".into(),
        child_mounts: false,
        capabilities: capabilities(false),
        ownership: p::Ownership::Host,
        settings_schema: None,
        instance_schema: Some(
            serde_json::from_value(serde_json::json!({
                "type": "object",
                "properties": {
                    "profile": {"type": "string", "x-immutable": true},
                    "gate": {"type": "string", "x-immutable": true}
                },
                "additionalProperties": false
            }))
            .expect("constant schema"),
        ),
        config_version: 1,
    }
}

#[derive(Clone)]
pub struct Config {
    pub profile: String,
    pub gate: Option<String>,
}
pub fn prepare(config: &Object) -> Result<Config> {
    for field in config.keys() {
        if field != "profile" && field != "gate" {
            return Err(p::Failure {
                code: "invalid_instance_config".into(),
                field: Some(field.clone()),
                config_kind: Some("unknown_field".into()),
            });
        }
    }
    let text = |field: &str| -> Result<Option<String>> {
        config
            .get(field)
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| p::Failure {
                    code: "invalid_instance_config".into(),
                    field: Some(field.into()),
                    config_kind: Some("invalid_type".into()),
                })
            })
            .transpose()
    };
    let profile = text("profile")?.unwrap_or_else(|| "full".into());
    if ![
        "full",
        "read-only",
        "escalated",
        "zero-chunk",
        "malformed-info",
        "drop-trap",
    ]
    .contains(&profile.as_str())
    {
        return Err(p::Failure {
            code: "invalid_instance_config".into(),
            field: Some("profile".into()),
            config_kind: Some("invalid_value".into()),
        });
    }
    Ok(Config {
        profile,
        gate: text("gate")?,
    })
}

#[derive(Default)]
struct Counters {
    storage: AtomicU64,
    reader: AtomicU64,
    writer: AtomicU64,
    storage_dropped: AtomicU64,
    reader_dropped: AtomicU64,
    writer_dropped: AtomicU64,
    commits: AtomicU64,
}
pub struct World {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    counters: Counters,
}
impl World {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            files: Mutex::new(BTreeMap::from([
                ("seed".into(), SEED.to_vec()),
                ("wait-read".into(), SEED.to_vec()),
                ("fail-next".into(), SEED.to_vec()),
            ])),
            counters: Counters::default(),
        })
    }
    fn metric(&self, name: &str) -> Option<u64> {
        let c = &self.counters;
        let counter = match name {
            "storage" => &c.storage,
            "reader" => &c.reader,
            "writer" => &c.writer,
            "storage-dropped" => &c.storage_dropped,
            "reader-dropped" => &c.reader_dropped,
            "writer-dropped" => &c.writer_dropped,
            "commits" => &c.commits,
            _ => return None,
        };
        Some(counter.load(Ordering::SeqCst))
    }
}
enum Kind {
    Storage,
    Reader,
    Writer,
}
struct Count {
    world: Arc<World>,
    kind: Kind,
}
impl Count {
    fn new(world: &Arc<World>, kind: Kind) -> Self {
        match kind {
            Kind::Storage => &world.counters.storage,
            Kind::Reader => &world.counters.reader,
            Kind::Writer => &world.counters.writer,
        }
        .fetch_add(1, Ordering::SeqCst);
        Self {
            world: world.clone(),
            kind,
        }
    }
}
impl Drop for Count {
    fn drop(&mut self) {
        let c = &self.world.counters;
        let (live, dropped) = match self.kind {
            Kind::Storage => (&c.storage, &c.storage_dropped),
            Kind::Reader => (&c.reader, &c.reader_dropped),
            Kind::Writer => (&c.writer, &c.writer_dropped),
        };
        live.fetch_sub(1, Ordering::SeqCst);
        dropped.fetch_add(1, Ordering::SeqCst);
    }
}
pub fn entry(path: &str, size: Option<u64>) -> Entry {
    Entry {
        path: path.into(),
        kind: if size.is_none() {
            EntryKind::Dir
        } else {
            EntryKind::File
        },
        size,
        modified: Some(p::Timestamp {
            seconds: -1,
            nanos: 999_999_999,
        }),
    }
}
pub struct Storage {
    world: Arc<World>,
    pub config: Config,
    _count: Count,
}
impl Storage {
    pub fn new(world: Arc<World>, config: Config) -> Self {
        let count = Count::new(&world, Kind::Storage);
        Self {
            world,
            config,
            _count: count,
        }
    }
    pub fn info(&self) -> p::StorageInfo {
        let mut caps = capabilities(self.config.profile == "read-only");
        // Unknown optional names must not accidentally grant capabilities.
        caps.push("future.optional.capability".into());
        if self.config.profile == "escalated" {
            caps.push("create_dir".into());
        }
        p::StorageInfo {
            capabilities: caps,
            chunk_size: if self.config.profile == "zero-chunk" {
                0
            } else {
                CHUNK
            },
        }
    }
    pub fn stat(&self, path: &str) -> Result<Entry> {
        if path == "panic" {
            panic!("conformance injected read panic");
        }
        if let Some(code) = path.strip_prefix("error-") {
            return Err(failure(code));
        }
        if let Some(metric) = path
            .strip_prefix("__stats-")
            .and_then(|name| self.world.metric(name))
        {
            return Ok(entry(path, Some(metric)));
        }
        if path.is_empty() {
            return Ok(entry(path, None));
        }
        self.world
            .files
            .lock()
            .expect("fixture lock")
            .get(path)
            .map(|bytes| entry(path, Some(bytes.len() as u64)))
            .ok_or_else(|| failure("not_found"))
    }
    pub fn list(
        &self,
        path: &str,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Vec<Entry>, Option<String>)> {
        if !path.is_empty() {
            self.stat(path)?;
            return Err(failure("not_dir"));
        }
        if limit == 0 {
            return Err(failure("invalid_request"));
        }
        let files = self.world.files.lock().expect("fixture lock");
        let mut entries: Vec<_> = files
            .iter()
            .filter(|(name, _)| after.is_none_or(|after| name.as_str() > after))
            .take(limit as usize + 1)
            .map(|(name, bytes)| entry(name, Some(bytes.len() as u64)))
            .collect();
        let more = entries.len() > limit as usize;
        if more {
            entries.pop();
        }
        let next = more.then(|| entries.last().expect("nonempty page").path.clone());
        Ok((entries, next))
    }
    pub fn read(&self, path: &str, offset: u64, length: Option<u64>) -> Result<Reader> {
        let files = self.world.files.lock().expect("fixture lock");
        let data = files.get(path).ok_or_else(|| {
            failure(if path.is_empty() {
                "is_dir"
            } else {
                "not_found"
            })
        })?;
        let start = offset.min(data.len() as u64) as usize;
        let end = start
            .saturating_add(length.unwrap_or(u64::MAX).min(usize::MAX as u64) as usize)
            .min(data.len());
        Ok(Reader {
            data: data[start..end].into(),
            position: 0,
            fail: path == "fail-next",
            _count: Count::new(&self.world, Kind::Reader),
        })
    }
    pub fn write(&self, path: &str, replace: bool) -> Result<Writer> {
        if self.config.profile == "read-only" {
            return Err(failure("unsupported_operation"));
        }
        if path.is_empty() {
            return Err(failure("root_protected"));
        }
        if !replace
            && self
                .world
                .files
                .lock()
                .expect("fixture lock")
                .contains_key(path)
        {
            return Err(failure("already_exists"));
        }
        Ok(Writer {
            world: self.world.clone(),
            path: path.into(),
            replace,
            pending: Some(vec![]),
            count: Some(Count::new(&self.world, Kind::Writer)),
        })
    }
    pub fn remove(&self, path: &str) -> Result<()> {
        if self.config.profile == "read-only" {
            return Err(failure("unsupported_operation"));
        }
        if path.is_empty() {
            return Err(failure("root_protected"));
        }
        self.world
            .files
            .lock()
            .expect("fixture lock")
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| failure("not_found"))
    }
}
pub struct Reader {
    data: Vec<u8>,
    position: usize,
    fail: bool,
    _count: Count,
}
impl Reader {
    pub fn next_chunk(&mut self) -> Result<Option<Vec<u8>>> {
        if self.fail {
            return Err(failure("backend"));
        }
        if self.position == self.data.len() {
            return Ok(None);
        }
        let end = (self.position + CHUNK as usize).min(self.data.len());
        let bytes = self.data[self.position..end].into();
        self.position = end;
        Ok(Some(bytes))
    }
}
pub struct Writer {
    world: Arc<World>,
    pub path: String,
    replace: bool,
    pending: Option<Vec<u8>>,
    count: Option<Count>,
}
impl Writer {
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        if self.pending.is_none() {
            return Err(failure("session_closed"));
        }
        if self.path == "fail-write" {
            self.pending.take();
            self.count.take();
            return Err(failure("backend"));
        }
        self.pending
            .as_mut()
            .expect("checked session")
            .extend_from_slice(data);
        Ok(())
    }
    pub fn take_commit(&mut self) -> Result<Pending> {
        let bytes = self
            .pending
            .take()
            .ok_or_else(|| failure("session_closed"))?;
        self.count.take();
        Ok(Pending {
            world: self.world.clone(),
            path: self.path.clone(),
            replace: self.replace,
            bytes,
        })
    }
    pub fn abort(&mut self) -> Result<()> {
        self.pending
            .take()
            .ok_or_else(|| failure("session_closed"))?;
        self.count.take();
        if self.path == "fail-abort" {
            return Err(failure("backend"));
        }
        Ok(())
    }
}
pub struct Pending {
    world: Arc<World>,
    pub path: String,
    replace: bool,
    bytes: Vec<u8>,
}
impl Pending {
    pub fn publish(self) -> Result<Entry> {
        if self.path == "fail-commit" {
            return Err(failure("backend"));
        }
        let mut files = self.world.files.lock().expect("fixture lock");
        if !self.replace && files.contains_key(&self.path) {
            return Err(failure("already_exists"));
        }
        let size = self.bytes.len() as u64;
        files.insert(self.path.clone(), self.bytes);
        self.world.counters.commits.fetch_add(1, Ordering::SeqCst);
        drop(files);
        if self.path == "panic-commit" {
            panic!("conformance injected mutation panic");
        }
        if self.path == "uncertain-commit" {
            return Err(failure("outcome_unknown"));
        }
        Ok(entry(&self.path, Some(size)))
    }
}
