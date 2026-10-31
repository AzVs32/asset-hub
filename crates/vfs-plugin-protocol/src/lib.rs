//! Portable plugin values. This crate has no host database or runtime dependency.
//! Control values use lossless UTF-8 JSON; file data travels in separate binary
//! frames. gRPC and WIT provide framing and resource ownership, respectively.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Object = BTreeMap<String, serde_json::Value>;
pub const PROTOCOL_MAJOR: u32 = 1;
pub const PROTOCOL_MINOR: u32 = 1;
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_DATA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RESOURCES: usize = 1024;
pub const INSTANCE_SCHEMA_PROFILE: &str = "assethub.config.annotations.v1";
pub const WASM_HOST_PROFILE: &str = "assethub.service.v1";
pub const HOST_HTTP: &str = "http.v1";
pub const HOST_CLOCK: &str = "clock.wall.v1";
pub const HOST_RANDOM: &str = "random.v1";
pub const WASM_HOST_CAPABILITIES: &[&str] = &[HOST_HTTP, HOST_CLOCK, HOST_RANDOM];
pub const PROTOCOL_FEATURES: &[&str] = &[
    "instances.host-managed.v1",
    "instances.self-managed.v1",
    "streams.chunked.v1",
    "resources.leases.v1",
    INSTANCE_SCHEMA_PROFILE,
];

fn schema_profile() -> String {
    INSTANCE_SCHEMA_PROFILE.into()
}
fn baseline_features() -> Vec<String> {
    PROTOCOL_FEATURES.iter().map(|feature| (*feature).into()).collect()
}

/// Selected backward-compatible protocol revision. Minor revisions may add
/// optional data but cannot change existing operation meanings or encodings.
#[derive(Clone, Debug)]
pub struct NegotiatedProtocol {
    pub major: u32,
    pub minor: u32,
    pub features: Vec<String>,
    pub instance_schema_profile: String,
}

#[derive(Clone, Copy, Debug)]
pub enum IncompatiblePlugin {
    MajorVersion,
    RequiredFeature,
    SchemaProfile,
    HostCapability,
}

/// Check declarations before initialize. Host capabilities are imported runtime
/// services, distinct from backend Storage capabilities and runtime permissions.
pub fn negotiate(
    description: &Description,
    host_capabilities: &[&str],
) -> Result<NegotiatedProtocol, IncompatiblePlugin> {
    let negotiated = negotiate_protocol(description)?;
    if description.required_host_capabilities.iter()
        .any(|capability| !host_capabilities.contains(&capability.as_str())) {
        return Err(IncompatiblePlugin::HostCapability);
    }
    Ok(negotiated)
}

/// Validate the portable contract without assuming a particular runtime's
/// imports. Transports additionally call negotiate with their provided services.
pub fn negotiate_protocol(
    description: &Description,
) -> Result<NegotiatedProtocol, IncompatiblePlugin> {
    if description.major != PROTOCOL_MAJOR {
        return Err(IncompatiblePlugin::MajorVersion);
    }
    if description.required_features.iter().any(|feature| {
        !PROTOCOL_FEATURES.contains(&feature.as_str())
            || !description.features.contains(feature)
    }) {
        return Err(IncompatiblePlugin::RequiredFeature);
    }
    if description.instance_schema_profile != INSTANCE_SCHEMA_PROFILE {
        return Err(IncompatiblePlugin::SchemaProfile);
    }
    Ok(NegotiatedProtocol {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR.min(description.minor),
        features: description.features.iter()
            .filter(|feature| PROTOCOL_FEATURES.contains(&feature.as_str()))
            .cloned().collect(),
        instance_schema_profile: description.instance_schema_profile.clone(),
    })
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    Host,
    Plugin,
}

/// Capabilities use named strings, not compiler-assigned enum ordinals.
/// Unknown names do not grant support. Permission/xattr reads and writes have
/// separate names; directory models and read modes remain exclusive.
pub type Capabilities = Vec<String>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Description {
    pub major: u32,
    pub minor: u32,
    pub required_features: Vec<String>,
    #[serde(default = "baseline_features")]
    pub features: Vec<String>,
    #[serde(default)]
    pub required_host_capabilities: Vec<String>,
    #[serde(default = "schema_profile")]
    pub instance_schema_profile: String,
    pub generation: String,
    pub key: String,
    pub name: String,
    pub version: String,
    pub child_mounts: bool,
    pub capabilities: Capabilities,
    pub ownership: Ownership,
    pub settings_schema: Option<Object>,
    pub instance_schema: Option<Object>,
    pub config_version: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub capabilities: Capabilities,
    pub chunk_size: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Timestamp {
    pub seconds: i64,
    pub nanos: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    pub modified: Option<Timestamp>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub details: Object,
}

/// Error fields contain categories and safe field identifiers only. Transport
/// adapters must not serialize backend diagnostic chains or request values.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    pub code: String,
    pub field: Option<String>,
    pub config_kind: Option<String>,
}
impl Failure {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.into(),
            field: None,
            config_kind: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum StorageOp {
    Stat {
        path: String,
    },
    List {
        path: String,
        after: Option<String>,
        limit: u32,
    },
    CreateDir {
        path: String,
        recursive: bool,
    },
    Remove {
        path: String,
    },
    Move {
        source: String,
        destination: String,
        replace: bool,
    },
    GetPermissions {
        path: String,
    },
    SetPermissions {
        path: String,
        mode: u32,
    },
    ListXattrs {
        path: String,
        max_bytes: u32,
    },
    GetXattr {
        path: String,
        name: String,
        max_bytes: u32,
    },
    SetXattr {
        path: String,
        name: String,
        mode: String,
    },
    RemoveXattr {
        path: String,
        name: String,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Operation {
    Describe,
    Initialize {
        settings: Object,
    },
    Prepare {
        config: Object,
    },
    Migrate {
        from_version: u32,
        config: Object,
    },
    TestConnection {
        config: Object,
    },
    GetInstance {
        id: String,
    },
    ListInstances {
        after: Option<String>,
        limit: u32,
    },
    CreateInstance {
        config: Object,
    },
    RemoveInstance {
        id: String,
    },
    Open {
        id: String,
        config: Option<Object>,
    },
    Storage {
        handle: u64,
        request: StorageOp,
    },
    Read {
        handle: u64,
        path: String,
        offset: u64,
        length: Option<u64>,
    },
    Write {
        handle: u64,
        path: String,
        replace: bool,
        atomic: bool,
    },
    ReaderNext {
        handle: u64,
    },
    WriterChunk {
        handle: u64,
    },
    Commit {
        handle: u64,
    },
    Abort {
        handle: u64,
    },
    Release {
        kind: ResourceKind,
        handle: u64,
    },
    Renew {
        resources: Vec<ResourceRef>,
    },
}
impl Operation {
    /// An ambiguous failure after dispatch of these operations cannot prove rollback.
    pub fn mutates(&self) -> bool {
        match self {
            Self::Initialize { .. }
            | Self::CreateInstance { .. }
            | Self::RemoveInstance { .. }
            | Self::Write { .. }
            | Self::WriterChunk { .. }
            | Self::Commit { .. }
            | Self::Abort { .. } => true,
            Self::Storage { request, .. } => matches!(
                request,
                StorageOp::CreateDir { .. }
                    | StorageOp::Remove { .. }
                    | StorageOp::Move { .. }
                    | StorageOp::SetPermissions { .. }
                    | StorageOp::SetXattr { .. }
                    | StorageOp::RemoveXattr { .. }
            ),
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Storage,
    Reader,
    Writer,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ResourceRef {
    pub kind: ResourceKind,
    pub handle: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Value {
    Description {
        description: Description,
    },
    Prepared {
        config: Object,
        details: Object,
    },
    Config {
        config: Object,
    },
    Instance {
        instance: Instance,
    },
    MaybeInstance {
        instance: Option<Instance>,
    },
    Instances {
        instances: Vec<Instance>,
        next_after: Option<String>,
    },
    Removed {
        removed: bool,
    },
    Storage {
        handle: u64,
        info: StorageInfo,
    },
    Reader {
        handle: u64,
    },
    Writer {
        handle: u64,
    },
    Entry {
        entry: Entry,
    },
    Page {
        entries: Vec<Entry>,
        next_after: Option<String>,
    },
    Permissions {
        mode: u32,
    },
    Names {
        names: Vec<String>,
    },
    Bytes,
    Chunk {
        eof: bool,
    },
    Unit,
}

/// Binary data is deliberately excluded from control serialization.
pub struct Reply {
    pub value: Value,
    pub data: Vec<u8>,
}
impl Reply {
    pub fn new(value: Value) -> Self {
        Self {
            value,
            data: Vec::new(),
        }
    }
    pub fn unit() -> Self {
        Self::new(Value::Unit)
    }
}
