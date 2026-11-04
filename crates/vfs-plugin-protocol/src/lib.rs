//! Shared protobuf messages, validation and VFS protocol semantics.
pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/assethub.vfs.v3.rs"));
}
pub use wire::*;
pub use wire::{reply::Result as Value, request::Operation, storage_call::Operation as StorageOp};
mod codec;
pub use codec::*;
/// Protoc descriptor set for language-independent tooling and generators.
pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plugin.bin"));
/// JSON values at application/configuration boundaries, never a wire message.
pub type JsonObject = std::collections::BTreeMap<String, serde_json::Value>;

pub const PROTOCOL_MAJOR: u32 = 3;
pub const PROTOCOL_MINOR: u32 = 0;
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_DATA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FRAME_BYTES: usize = MAX_CONTROL_BYTES + MAX_DATA_BYTES + 1024;
pub const MAX_RESOURCES: usize = 1024;
pub const INSTANCE_SCHEMA_PROFILE: &str = "assethub.config.annotations.v1";
pub const PROTOCOL_FEATURES: &[&str] = &["streams.chunked.v1"];

/// Selected backward-compatible protocol revision. Minor revisions may add
/// optional data but cannot change existing operation meanings or encodings.
#[derive(Clone, Debug, PartialEq)]
pub struct NegotiatedProtocol {
    pub major: u32,
    pub minor: u32,
    pub features: Vec<String>,
    pub instance_schema_profile: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IncompatiblePlugin {
    MajorVersion,
    RequiredFeature,
    SchemaProfile,
    HostCapabilities(Vec<String>),
}

impl std::fmt::Display for IncompatiblePlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MajorVersion => f.write_str("incompatible plugin protocol major version"),
            Self::RequiredFeature => f.write_str("unsupported required plugin feature"),
            Self::SchemaProfile => f.write_str("unsupported plugin schema profile"),
            Self::HostCapabilities(missing) => {
                write!(f, "ungranted host capabilities: {}", missing.join(", "))
            }
        }
    }
}
impl std::error::Error for IncompatiblePlugin {}

/// Check declarations before initialize. Host capabilities are imported runtime
/// services, distinct from backend Storage capabilities and runtime permissions.
pub fn negotiate(
    description: &Description,
    host_capabilities: &[&str],
) -> Result<NegotiatedProtocol, IncompatiblePlugin> {
    let negotiated = negotiate_protocol(description)?;
    let missing = description
        .required_host_capabilities
        .iter()
        .filter(|capability| !host_capabilities.contains(&capability.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(IncompatiblePlugin::HostCapabilities(missing));
    }
    Ok(negotiated)
}

/// Validate the portable contract without assuming a particular runtime's
/// imports. The host additionally negotiates the services exposed by its transport.
pub fn negotiate_protocol(
    description: &Description,
) -> Result<NegotiatedProtocol, IncompatiblePlugin> {
    if description.major != PROTOCOL_MAJOR {
        return Err(IncompatiblePlugin::MajorVersion);
    }
    if description.required_features.iter().any(|feature| {
        !PROTOCOL_FEATURES.contains(&feature.as_str()) || !description.features.contains(feature)
    }) {
        return Err(IncompatiblePlugin::RequiredFeature);
    }
    if description.instance_schema_profile != INSTANCE_SCHEMA_PROFILE {
        return Err(IncompatiblePlugin::SchemaProfile);
    }
    Ok(NegotiatedProtocol {
        major: PROTOCOL_MAJOR,
        // The initial v2 revision is zero, so every peer minor negotiates to zero.
        minor: PROTOCOL_MINOR,
        features: description
            .features
            .iter()
            .filter(|feature| PROTOCOL_FEATURES.contains(&feature.as_str()))
            .cloned()
            .collect(),
        instance_schema_profile: description.instance_schema_profile.clone(),
    })
}

impl Failure {
    pub fn new(code: FailureCode) -> Self {
        Self {
            code: code as i32,
            field: None,
            config_kind: None,
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "plugin failure: {}", self.code().as_str_name())
    }
}
impl std::error::Error for Failure {}
impl Reply {
    pub fn new(value: Value) -> Self {
        Self {
            result: Some(value),
            data: vec![],
        }
    }
    pub fn unit() -> Self {
        Self::new(Value::Unit(Empty {}))
    }
    pub fn into_value(self) -> Result<Value, Failure> {
        match self.result {
            Some(Value::Failure(error)) => Err(error),
            Some(value) => Ok(value),
            None => Err(Failure::new(FailureCode::InvalidData)),
        }
    }
}
impl Operation {
    /// Operations scoped to a live Storage rather than the driver control RPC.
    pub fn is_resource(&self) -> bool {
        matches!(
            self,
            Self::Storage(_)
                | Self::Read(_)
                | Self::Write(_)
                | Self::ReaderNext(_)
                | Self::WriterChunk(_)
                | Self::Commit(_)
                | Self::Abort(_)
                | Self::Release(_)
        )
    }
    /// Check the result alternative before exposing a transport response.
    pub fn accepts_reply(&self, value: &Value) -> bool {
        match self {
            Self::Describe(_) => matches!(value, Value::Description(_)),
            Self::Initialize(_)
            | Self::TestConnection(_)
            | Self::WriterChunk(_)
            | Self::Abort(_)
            | Self::Release(_) => matches!(value, Value::Unit(_)),
            Self::Prepare(_) => matches!(value, Value::Prepared(_)),
            Self::Migrate(_) => matches!(value, Value::Config(_)),

            Self::Open(_) => matches!(value, Value::Storage(_)),
            Self::Read(_) => matches!(value, Value::Reader(_)),
            Self::Write(_) => matches!(value, Value::Writer(_)),
            Self::ReaderNext(_) => matches!(value, Value::Chunk(_)),
            Self::Commit(_) => matches!(value, Value::Entry(_)),
            Self::Storage(call) => match &call.operation {
                Some(StorageOp::Stat(_) | StorageOp::CreateDir(_) | StorageOp::Move(_)) => {
                    matches!(value, Value::Entry(_))
                }
                Some(StorageOp::List(_)) => matches!(value, Value::Page(_)),
                Some(StorageOp::GetPermissions(_)) => matches!(value, Value::Permissions(_)),
                Some(StorageOp::ListXattrs(_)) => matches!(value, Value::Names(_)),
                Some(StorageOp::GetXattr(_)) => matches!(value, Value::Bytes(_)),
                Some(
                    StorageOp::Remove(_)
                    | StorageOp::SetPermissions(_)
                    | StorageOp::SetXattr(_)
                    | StorageOp::RemoveXattr(_),
                ) => matches!(value, Value::Unit(_)),
                None => false,
            },
        }
    }
    /// Ambiguous failures cannot prove that these mutations were rolled back.
    pub fn mutates(&self) -> bool {
        match self {
            Self::Initialize(_)
            | Self::Write(_)
            | Self::WriterChunk(_)
            | Self::Commit(_)
            | Self::Abort(_) => true,
            Self::Storage(call) => matches!(
                call.operation,
                Some(
                    StorageOp::CreateDir(_)
                        | StorageOp::Remove(_)
                        | StorageOp::Move(_)
                        | StorageOp::SetPermissions(_)
                        | StorageOp::SetXattr(_)
                        | StorageOp::RemoveXattr(_)
                )
            ),
            _ => false,
        }
    }
}
