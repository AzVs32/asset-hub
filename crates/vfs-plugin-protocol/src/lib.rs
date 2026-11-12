//! Shared protobuf messages, validation and VFS protocol semantics.
pub mod wire {
    include!(concat!(env!("OUT_DIR"), "/assethub.vfs.v14.rs"));
}
pub use wire::*;
pub use wire::{reply::Result as Value, request::Operation, storage_call::Operation as StorageOp};
mod attributes;
mod codec;
mod directory;
mod lookup;
mod values;
pub use attributes::{
    AttributeValueError, decode_attribute, decode_attribute_result, encode_attribute,
    encode_attribute_result,
};
pub use codec::*;
pub use directory::*;
pub use lookup::{
    LookupValueError, decode_create_directory, decode_lookup, decode_read, decode_remove,
    decode_rename, decode_stat, decode_write, encode_create_directory, encode_link_target,
    encode_lookup, encode_read, encode_remove, encode_rename, encode_stat, encode_write,
};
pub use values::{
    EntryValueError, decode_capabilities, decode_entry, decode_time, encode_capabilities,
    encode_entry, encode_time,
};
/// Protoc descriptor set for language-independent tooling and generators.
pub const FILE_DESCRIPTOR_SET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plugin.bin"));
/// JSON values at application/configuration boundaries, never a wire message.
pub type JsonObject = std::collections::BTreeMap<String, serde_json::Value>;

pub const PROTOCOL_MAJOR: u32 = 14;
pub const PROTOCOL_MINOR: u32 = 0;
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_DATA_BYTES: usize = 8 * 1024 * 1024;
/// Bound the allocation for converting a packed name list into typed keys.
pub const MAX_ATTRIBUTE_NAMES_BYTES: usize = MAX_CONTROL_BYTES;
pub const MAX_FRAME_BYTES: usize = MAX_CONTROL_BYTES + MAX_DATA_BYTES + 1024;
pub const MAX_RESOURCES: usize = 1024;
/// Bound the number of retained observations in one lookup.
pub const MAX_LOOKUP_ENTRIES: usize = 128 * 1024;
pub const INSTANCE_SCHEMA_PROFILE: &str = "assethub.config.annotations.v1";
pub const PROTOCOL_FEATURES: &[&str] = &["streams.chunked.v1"];

/// Wire metadata shared by all production descriptions.
pub fn description_header(generation: String) -> Description {
    Description {
        major: PROTOCOL_MAJOR,
        minor: PROTOCOL_MINOR,
        features: PROTOCOL_FEATURES
            .iter()
            .map(|name| (*name).into())
            .collect(),
        instance_schema_profile: INSTANCE_SCHEMA_PROFILE.into(),
        generation,
        ..Default::default()
    }
}

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
        // This major defines only minor revision zero.
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
            storage: None,
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
            None => Err(Failure::new(FailureCode::ProtocolViolation)),
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
                | Self::BeginLookup(_)
                | Self::ObserveChild(_)
                | Self::ReadObservedLink(_)
                | Self::VerifyLookup(_)
                | Self::Release(_)
        )
    }
    /// Whether the reply can allocate a new resource in this session.
    pub fn allocates_resource(&self) -> bool {
        matches!(
            self,
            Self::Open(_) | Self::Read(_) | Self::Write(_) | Self::BeginLookup(_)
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
            | Self::VerifyLookup(_)
            | Self::Release(_) => matches!(value, Value::Unit(_)),
            Self::Prepare(_) => matches!(value, Value::Prepared(_)),
            Self::Migrate(_) => matches!(value, Value::Config(_)),

            Self::Open(_) => matches!(value, Value::Storage(_)),
            Self::Read(_) => matches!(value, Value::Reader(_)),
            Self::Write(_) => matches!(value, Value::Writer(_)),
            Self::ReaderNext(_) => matches!(value, Value::Chunk(_)),
            Self::Commit(_) => matches!(value, Value::Entry(_)),
            Self::BeginLookup(_) => matches!(value, Value::OpenedLookup(_)),
            Self::ObserveChild(_) => matches!(value, Value::ObservedEntry(_)),
            Self::ReadObservedLink(_) => matches!(value, Value::Bytes(_)),
            Self::Storage(call) => match &call.operation {
                Some(StorageOp::Stat(_) | StorageOp::CreateDirectory(_) | StorageOp::Rename(_)) => {
                    matches!(value, Value::Entry(_))
                }
                Some(StorageOp::List(_)) => matches!(value, Value::Page(_)),
                Some(StorageOp::SelectDirectory(_)) => matches!(value, Value::SelectedDirectory(_)),
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
}

pub fn decode_entry_slot(
    slot: &EntrySlot,
) -> Result<(ah_vfs_storage::EntryName, ah_vfs_storage::StoragePath), CodecError> {
    if slot.parent == 0 {
        return Err(CodecError::InvalidData);
    }
    let name = ah_vfs_storage::EntryName::parse(&slot.name).map_err(|_| CodecError::InvalidData)?;
    let path =
        ah_vfs_storage::StoragePath::parse(&slot.path).map_err(|_| CodecError::InvalidData)?;
    Ok((name, path))
}

mod failure;
pub use failure::{
    decode_lookup_purpose, decode_storage_error, encode_lookup_purpose, encode_storage_error,
};

impl StorageOp {
    pub fn storage_operation(&self) -> ah_vfs_storage::Operation {
        use ah_vfs_storage::Operation as D;
        match self {
            Self::Stat(_) => D::Stat,
            Self::List(_) | Self::SelectDirectory(_) => D::List,
            Self::CreateDirectory(_) => D::CreateDirectory,
            Self::Remove(_) => D::Remove,
            Self::Rename(_) => D::Rename,
            Self::GetPermissions(_) => D::GetPermissions,
            Self::SetPermissions(_) => D::SetPermissions,
            Self::ListXattrs(_) => D::ListXattrs,
            Self::GetXattr(_) => D::GetXattr,
            Self::SetXattr(_) => D::SetXattr,
            Self::RemoveXattr(_) => D::RemoveXattr,
        }
    }
}
impl Operation {
    pub fn storage_operation(&self) -> Option<ah_vfs_storage::Operation> {
        use ah_vfs_storage::Operation as D;
        match self {
            Self::Storage(call) => call.operation.as_ref().map(StorageOp::storage_operation),
            Self::Read(_) | Self::ReaderNext(_) => Some(D::Read),
            Self::BeginLookup(call) | Self::VerifyLookup(call) => {
                decode_lookup_purpose(&call.purpose)
            }
            Self::ObserveChild(call) => decode_lookup_purpose(&call.purpose),
            Self::ReadObservedLink(call) => decode_lookup_purpose(&call.purpose),
            Self::Write(_) | Self::WriterChunk(_) | Self::Commit(_) | Self::Abort(_) => {
                Some(D::Write)
            }
            _ => None,
        }
    }
    /// Private staging and abort cannot publish covered state.
    pub fn may_publish(&self) -> bool {
        matches!(self, Self::Commit(_))
            || matches!(self, Self::Storage(call) if call.operation.as_ref().is_some_and(|op| op.storage_operation().may_mutate()))
    }
}
