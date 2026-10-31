//! Checked conversion between portable values and shared VFS domain types.
use ah_vfs::{
    application::{driver_info::DriverInfo, dto::*, port::*},
    domain::*,
};
use std::{
    collections::BTreeSet,
    num::{NonZeroU32, NonZeroUsize},
    time::{Duration, UNIX_EPOCH},
};
use vfs_plugin_protocol as p;

pub fn invalid() -> DriverError {
    StorageError::new(StorageErrorKind::InvalidData).into()
}
pub fn storage_invalid() -> StorageError {
    StorageError::new(StorageErrorKind::InvalidData)
}
pub fn failure(error: DriverError) -> p::Failure {
    match error {
        DriverError::Storage(error) => p::Failure::new(error_code(error.kind())),
        DriverError::InstanceNotFound(_) => p::Failure::new("instance_not_found"),
        DriverError::InvalidInstanceConfig { field, kind } => p::Failure {
            code: "invalid_instance_config".into(),
            field: Some(field),
            config_kind: Some(
                match kind {
                    InstanceConfigErrorKind::MissingField => "missing_field",
                    InstanceConfigErrorKind::UnknownField => "unknown_field",
                    InstanceConfigErrorKind::InvalidType => "invalid_type",
                    InstanceConfigErrorKind::InvalidValue => "invalid_value",
                }
                .into(),
            ),
        },
    }
}
pub fn error_code(kind: StorageErrorKind) -> &'static str {
    match kind {
        StorageErrorKind::NotFound => "not_found",
        StorageErrorKind::AlreadyExists => "already_exists",
        StorageErrorKind::PermissionDenied => "permission_denied",
        StorageErrorKind::NotDir => "not_dir",
        StorageErrorKind::IsDir => "is_dir",
        StorageErrorKind::DirNotEmpty => "dir_not_empty",
        StorageErrorKind::InvalidPath => "invalid_path",
        StorageErrorKind::InvalidRequest => "invalid_request",
        StorageErrorKind::InvalidNameEncoding => "invalid_name_encoding",
        StorageErrorKind::InvalidData => "invalid_data",
        StorageErrorKind::RootProtected => "root_protected",
        StorageErrorKind::SessionClosed => "session_closed",
        StorageErrorKind::UnsupportedOperation => "unsupported_operation",
        StorageErrorKind::StaleHandle => "stale_handle",
        StorageErrorKind::TimedOut => "timed_out",
        StorageErrorKind::Unavailable => "unavailable",
        StorageErrorKind::RateLimited => "rate_limited",
        StorageErrorKind::Conflict => "conflict",
        StorageErrorKind::CapacityExceeded => "capacity_exceeded",
        StorageErrorKind::CrossDevice => "cross_device",
        StorageErrorKind::Cancelled => "cancelled",
        StorageErrorKind::OutcomeUnknown => "outcome_unknown",
        StorageErrorKind::AttributeNotFound => "attribute_not_found",
        StorageErrorKind::TooLarge => "too_large",
        StorageErrorKind::Backend => "backend",
        _ => "backend",
    }
}
pub fn error_kind(code: &str) -> StorageErrorKind {
    match code {
        "not_found" => StorageErrorKind::NotFound,
        "already_exists" => StorageErrorKind::AlreadyExists,
        "permission_denied" => StorageErrorKind::PermissionDenied,
        "not_dir" => StorageErrorKind::NotDir,
        "is_dir" => StorageErrorKind::IsDir,
        "dir_not_empty" => StorageErrorKind::DirNotEmpty,
        "invalid_path" => StorageErrorKind::InvalidPath,
        "invalid_request" => StorageErrorKind::InvalidRequest,
        "invalid_name_encoding" => StorageErrorKind::InvalidNameEncoding,
        "invalid_data" => StorageErrorKind::InvalidData,
        "root_protected" => StorageErrorKind::RootProtected,
        "session_closed" => StorageErrorKind::SessionClosed,
        "unsupported_operation" => StorageErrorKind::UnsupportedOperation,
        "stale_handle" => StorageErrorKind::StaleHandle,
        "timed_out" => StorageErrorKind::TimedOut,
        "unavailable" => StorageErrorKind::Unavailable,
        "rate_limited" => StorageErrorKind::RateLimited,
        "conflict" => StorageErrorKind::Conflict,
        "capacity_exceeded" => StorageErrorKind::CapacityExceeded,
        "cross_device" => StorageErrorKind::CrossDevice,
        "cancelled" => StorageErrorKind::Cancelled,
        "outcome_unknown" => StorageErrorKind::OutcomeUnknown,
        "attribute_not_found" => StorageErrorKind::AttributeNotFound,
        "too_large" => StorageErrorKind::TooLarge,
        "backend" => StorageErrorKind::Backend,
        _ => StorageErrorKind::Backend,
    }
}
pub fn driver_error(error: p::Failure, instance: Option<&str>) -> DriverError {
    match error.code.as_str() {
        "instance_not_found" => match instance.and_then(|id| InstanceId::parse(id).ok()) {
            Some(id) => DriverError::InstanceNotFound(id),
            None => invalid(),
        },
        "invalid_instance_config" => {
            let kind = match error.config_kind.as_deref() {
                Some("missing_field") => InstanceConfigErrorKind::MissingField,
                Some("unknown_field") => InstanceConfigErrorKind::UnknownField,
                Some("invalid_type") => InstanceConfigErrorKind::InvalidType,
                Some("invalid_value") => InstanceConfigErrorKind::InvalidValue,
                _ => return invalid(),
            };
            DriverError::InvalidInstanceConfig {
                field: error.field.unwrap_or_default(),
                kind,
            }
        }
        _ => StorageError::new(error_kind(&error.code)).into(),
    }
}
pub fn capabilities(c: StorageCapabilities) -> p::Capabilities {
    let mut names = Vec::new();
    for (enabled, name) in [
        (c.stat_entry, "stat"),
        (c.list_entries, "list"),
        (c.create_dir, "create_dir"),
        (c.remove_entry, "remove"),
        (c.persistent_empty_dirs, "empty_dirs"),
        (c.write_file.create_new, "write_new"),
        (c.write_file.replace, "write_replace"),
        (c.write_file.atomic_publish, "atomic_publish"),
        (c.rename.no_replace, "rename_new"),
        (c.rename.replace, "rename_replace"),
        (c.permissions.can_read(), "permissions_read"),
        (c.permissions.can_write(), "permissions_write"),
        (c.xattrs.can_read(), "xattrs_read"),
        (c.xattrs.can_write(), "xattrs_write"),
    ] {
        if enabled {
            names.push(name.into());
        }
    }
    match c.read_file {
        FileReadSupport::None => (),
        FileReadSupport::Sequential => names.push("read".into()),
        FileReadSupport::Range => names.push("range_read".into()),
    }
    match c.dir_model {
        DirModel::None => (),
        DirModel::Hierarchical => names.push("hierarchical".into()),
        DirModel::Prefix => names.push("prefix".into()),
    }
    names
}
pub fn decode_capabilities(names: &[String]) -> Result<StorageCapabilities, DriverError> {
    let set: BTreeSet<_> = names.iter().map(String::as_str).collect();
    if set.len() != names.len()
        || (set.contains("prefix") && set.contains("hierarchical"))
        || (set.contains("range_read") && set.contains("read"))
        || (set.contains("permissions_write") && !set.contains("permissions_read"))
        || (set.contains("xattrs_write") && !set.contains("xattrs_read"))
    {
        return Err(invalid());
    }
    let attr = |r, w| {
        if set.contains(w) {
            AttributeSupport::ReadWrite
        } else if set.contains(r) {
            AttributeSupport::Read
        } else {
            AttributeSupport::None
        }
    };
    Ok(StorageCapabilities {
        stat_entry: set.contains("stat"),
        list_entries: set.contains("list"),
        read_file: if set.contains("range_read") {
            FileReadSupport::Range
        } else if set.contains("read") {
            FileReadSupport::Sequential
        } else {
            FileReadSupport::None
        },
        write_file: FileWriteSupport {
            create_new: set.contains("write_new"),
            replace: set.contains("write_replace"),
            atomic_publish: set.contains("atomic_publish"),
        },
        dir_model: if set.contains("hierarchical") {
            DirModel::Hierarchical
        } else if set.contains("prefix") {
            DirModel::Prefix
        } else {
            DirModel::None
        },
        persistent_empty_dirs: set.contains("empty_dirs"),
        create_dir: set.contains("create_dir"),
        remove_entry: set.contains("remove"),
        rename: RenameSupport {
            no_replace: set.contains("rename_new"),
            replace: set.contains("rename_replace"),
        },
        permissions: attr("permissions_read", "permissions_write"),
        xattrs: attr("xattrs_read", "xattrs_write"),
    })
}
pub fn describe(driver: &dyn Driver, generation: String) -> p::Description {
    let info = driver.info();
    p::Description {
        major: p::PROTOCOL_MAJOR,
        minor: p::PROTOCOL_MINOR,
        required_features: vec![],
        features: p::PROTOCOL_FEATURES.iter().map(|feature| (*feature).into()).collect(),
        required_host_capabilities: vec![],
        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
        generation,
        key: info.key().as_str().into(),
        name: info.name().clone(),
        version: info.version().to_string(),
        child_mounts: info.child_mount_policy() == ChildMountPolicy::Allow,
        capabilities: capabilities(info.capabilities()),
        ownership: match driver.instances() {
            InstanceManagement::HostManaged(_) => p::Ownership::Host,
            InstanceManagement::SelfManaged(_) => p::Ownership::Plugin,
        },
        settings_schema: info.settings_schema().cloned(),
        instance_schema: info.instance_config_schema().cloned(),
        config_version: info.instance_config_version().get(),
    }
}
pub fn driver_info(d: &p::Description) -> Result<DriverInfo, DriverError> {
    // Import availability is transport-specific; portable declarations are not.
    p::negotiate_protocol(d)
        .map_err(|_| DriverError::from(StorageError::new(StorageErrorKind::UnsupportedOperation)))?;
    if let Some(schema) = &d.instance_schema {
        ah_vfs::application::schema::validate_instance_config_schema(schema)?;
    }
    if d.generation.is_empty() || d.generation.len() > 128 || d.name.len() > 4096 {
        return Err(invalid());
    }
    Ok(DriverInfo::builder()
        .key(DriverKey::parse(&d.key).map_err(|_| invalid())?)
        .name(d.name.clone())
        .version(semver::Version::parse(&d.version).map_err(|_| invalid())?)
        .child_mount_policy(if d.child_mounts {
            ChildMountPolicy::Allow
        } else {
            ChildMountPolicy::Deny
        })
        .capabilities(decode_capabilities(&d.capabilities)?)
        .maybe_settings_schema(d.settings_schema.clone())
        .maybe_instance_config_schema(d.instance_schema.clone())
        .instance_config_version(NonZeroU32::new(d.config_version).ok_or_else(invalid)?)
        .build())
}
pub fn entry(e: StorageEntry) -> Result<p::Entry, DriverError> {
    let modified = e
        .modified
        .map(|time| {
            let (seconds, nanos) = match time.duration_since(UNIX_EPOCH) {
                Ok(d) => (
                    i64::try_from(d.as_secs()).map_err(|_| invalid())?,
                    d.subsec_nanos(),
                ),
                Err(e) => {
                    let d = e.duration();
                    let seconds = i64::try_from(d.as_secs()).map_err(|_| invalid())?;
                    if d.subsec_nanos() == 0 {
                        (-seconds, 0)
                    } else {
                        (-seconds - 1, 1_000_000_000 - d.subsec_nanos())
                    }
                }
            };
            Ok::<_, DriverError>(p::Timestamp { seconds, nanos })
        })
        .transpose()?;
    Ok(p::Entry {
        path: e.path.as_str().into(),
        kind: match e.kind {
            StorageEntryKind::File => p::EntryKind::File,
            StorageEntryKind::Dir => p::EntryKind::Dir,
            StorageEntryKind::Symlink => p::EntryKind::Symlink,
            StorageEntryKind::Other => p::EntryKind::Other,
        },
        size: e.size,
        modified,
    })
}
pub fn decode_entry(e: p::Entry) -> Result<StorageEntry, StorageError> {
    let modified = e
        .modified
        .map(|t| {
            if t.nanos >= 1_000_000_000 {
                return Err(storage_invalid());
            }
            let time = if t.seconds >= 0 {
                UNIX_EPOCH.checked_add(Duration::new(t.seconds as u64, t.nanos))
            } else if t.nanos == 0 {
                UNIX_EPOCH.checked_sub(Duration::from_secs(t.seconds.unsigned_abs()))
            } else {
                UNIX_EPOCH.checked_sub(Duration::new(
                    t.seconds.unsigned_abs() - 1,
                    1_000_000_000 - t.nanos,
                ))
            };
            time.ok_or_else(storage_invalid)
        })
        .transpose()?;
    if e.kind != p::EntryKind::File && e.size.is_some() {
        return Err(storage_invalid());
    }
    Ok(StorageEntry {
        path: StoragePath::parse(&e.path).map_err(|_| storage_invalid())?,
        kind: match e.kind {
            p::EntryKind::File => StorageEntryKind::File,
            p::EntryKind::Dir => StorageEntryKind::Dir,
            p::EntryKind::Symlink => StorageEntryKind::Symlink,
            p::EntryKind::Other => StorageEntryKind::Other,
        },
        size: e.size,
        modified,
    })
}
pub fn instance(i: InstanceInfo) -> p::Instance {
    p::Instance {
        id: i.id.as_str().into(),
        details: i.details,
    }
}
pub fn decode_instance(i: p::Instance) -> Result<InstanceInfo, DriverError> {
    Ok(InstanceInfo {
        id: InstanceId::parse(&i.id).map_err(|_| invalid())?,
        details: i.details,
    })
}
pub fn limit(value: u32) -> Result<NonZeroUsize, StorageError> {
    NonZeroUsize::new(value as usize)
        .ok_or_else(|| StorageError::new(StorageErrorKind::InvalidRequest))
}
pub fn storage_error(error: DriverError) -> StorageError {
    match error {
        DriverError::Storage(e) => e,
        _ => storage_invalid(),
    }
}
