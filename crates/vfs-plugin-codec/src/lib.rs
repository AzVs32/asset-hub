//! Checked conversion between portable values and shared VFS domain types.
use ah_vfs::{
    application::{driver_info::DriverInfo, port::*},
    domain::{
        DriverError, DriverKey, InstanceConfigErrorKind, InstanceId,
        namespace::ChildMountPolicy,
        storage::{
            AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
            StorageCapabilities, StorageEntry, StorageEntryKind, StorageError, StorageErrorKind,
            StoragePath,
        },
    },
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
        DriverError::InstanceNotFound(_) => p::Failure::new(p::FailureCode::InstanceNotFound),
        DriverError::InvalidInstanceConfig { field, kind } => p::Failure {
            code: p::FailureCode::InvalidInstanceConfig as i32,
            field: Some(field),
            config_kind: Some(
                match kind {
                    InstanceConfigErrorKind::MissingField => p::ConfigErrorKind::MissingField,
                    InstanceConfigErrorKind::UnknownField => p::ConfigErrorKind::UnknownField,
                    InstanceConfigErrorKind::InvalidType => p::ConfigErrorKind::InvalidType,
                    InstanceConfigErrorKind::InvalidValue => p::ConfigErrorKind::InvalidValue,
                }
                .into(),
            ),
        },
    }
}
pub fn error_code(kind: StorageErrorKind) -> p::FailureCode {
    match kind {
        StorageErrorKind::NotFound => p::FailureCode::NotFound,
        StorageErrorKind::AlreadyExists => p::FailureCode::AlreadyExists,
        StorageErrorKind::PermissionDenied => p::FailureCode::PermissionDenied,
        StorageErrorKind::NotDir => p::FailureCode::NotDir,
        StorageErrorKind::IsDir => p::FailureCode::IsDir,
        StorageErrorKind::DirNotEmpty => p::FailureCode::DirNotEmpty,
        StorageErrorKind::InvalidPath => p::FailureCode::InvalidPath,
        StorageErrorKind::InvalidRequest => p::FailureCode::InvalidRequest,
        StorageErrorKind::InvalidNameEncoding => p::FailureCode::InvalidNameEncoding,
        StorageErrorKind::InvalidData => p::FailureCode::InvalidData,
        StorageErrorKind::RootProtected => p::FailureCode::RootProtected,
        StorageErrorKind::SessionClosed => p::FailureCode::SessionClosed,
        StorageErrorKind::UnsupportedOperation => p::FailureCode::UnsupportedOperation,
        StorageErrorKind::StaleHandle => p::FailureCode::StaleHandle,
        StorageErrorKind::TimedOut => p::FailureCode::TimedOut,
        StorageErrorKind::Unavailable => p::FailureCode::Unavailable,
        StorageErrorKind::RateLimited => p::FailureCode::RateLimited,
        StorageErrorKind::Conflict => p::FailureCode::Conflict,
        StorageErrorKind::CapacityExceeded => p::FailureCode::CapacityExceeded,
        StorageErrorKind::CrossDevice => p::FailureCode::CrossDevice,
        StorageErrorKind::Cancelled => p::FailureCode::Cancelled,
        StorageErrorKind::OutcomeUnknown => p::FailureCode::OutcomeUnknown,
        StorageErrorKind::AttributeNotFound => p::FailureCode::AttributeNotFound,
        StorageErrorKind::TooLarge => p::FailureCode::TooLarge,
        StorageErrorKind::Backend => p::FailureCode::Backend,
        _ => p::FailureCode::Backend,
    }
}
pub fn error_kind(code: p::FailureCode) -> StorageErrorKind {
    match code {
        p::FailureCode::NotFound => StorageErrorKind::NotFound,
        p::FailureCode::AlreadyExists => StorageErrorKind::AlreadyExists,
        p::FailureCode::PermissionDenied => StorageErrorKind::PermissionDenied,
        p::FailureCode::NotDir => StorageErrorKind::NotDir,
        p::FailureCode::IsDir => StorageErrorKind::IsDir,
        p::FailureCode::DirNotEmpty => StorageErrorKind::DirNotEmpty,
        p::FailureCode::InvalidPath => StorageErrorKind::InvalidPath,
        p::FailureCode::InvalidRequest => StorageErrorKind::InvalidRequest,
        p::FailureCode::InvalidNameEncoding => StorageErrorKind::InvalidNameEncoding,
        p::FailureCode::InvalidData => StorageErrorKind::InvalidData,
        p::FailureCode::RootProtected => StorageErrorKind::RootProtected,
        p::FailureCode::SessionClosed => StorageErrorKind::SessionClosed,
        p::FailureCode::UnsupportedOperation => StorageErrorKind::UnsupportedOperation,
        p::FailureCode::StaleHandle => StorageErrorKind::StaleHandle,
        p::FailureCode::TimedOut => StorageErrorKind::TimedOut,
        p::FailureCode::Unavailable => StorageErrorKind::Unavailable,
        p::FailureCode::RateLimited => StorageErrorKind::RateLimited,
        p::FailureCode::Conflict => StorageErrorKind::Conflict,
        p::FailureCode::CapacityExceeded => StorageErrorKind::CapacityExceeded,
        p::FailureCode::CrossDevice => StorageErrorKind::CrossDevice,
        p::FailureCode::Cancelled => StorageErrorKind::Cancelled,
        p::FailureCode::OutcomeUnknown => StorageErrorKind::OutcomeUnknown,
        p::FailureCode::AttributeNotFound => StorageErrorKind::AttributeNotFound,
        p::FailureCode::TooLarge => StorageErrorKind::TooLarge,
        p::FailureCode::Backend => StorageErrorKind::Backend,
        _ => StorageErrorKind::Backend,
    }
}
pub fn driver_error(error: p::Failure, instance: Option<&str>) -> DriverError {
    match error.code() {
        p::FailureCode::InstanceNotFound => {
            match instance.and_then(|id| InstanceId::parse(id).ok()) {
                Some(id) => DriverError::InstanceNotFound(id),
                None => invalid(),
            }
        }
        p::FailureCode::InvalidInstanceConfig => {
            let kind = match error
                .config_kind
                .and_then(|v| p::ConfigErrorKind::try_from(v).ok())
            {
                Some(p::ConfigErrorKind::MissingField) => InstanceConfigErrorKind::MissingField,
                Some(p::ConfigErrorKind::UnknownField) => InstanceConfigErrorKind::UnknownField,
                Some(p::ConfigErrorKind::InvalidType) => InstanceConfigErrorKind::InvalidType,
                Some(p::ConfigErrorKind::InvalidValue) => InstanceConfigErrorKind::InvalidValue,
                _ => return invalid(),
            };
            DriverError::InvalidInstanceConfig {
                field: error.field.unwrap_or_default(),
                kind,
            }
        }
        _ => StorageError::new(error_kind(error.code())).into(),
    }
}
pub fn capabilities(c: StorageCapabilities) -> Vec<i32> {
    let mut names = Vec::new();
    for (enabled, name) in [
        (c.stat_entry, p::Capability::Stat),
        (c.list_entries, p::Capability::List),
        (c.create_dir, p::Capability::CreateDir),
        (c.remove_entry, p::Capability::Remove),
        (c.persistent_empty_dirs, p::Capability::EmptyDirs),
        (c.write_file.create_new, p::Capability::WriteNew),
        (c.write_file.replace, p::Capability::WriteReplace),
        (c.write_file.atomic_publish, p::Capability::AtomicPublish),
        (c.rename.no_replace, p::Capability::RenameNew),
        (c.rename.replace, p::Capability::RenameReplace),
        (c.permissions.can_read(), p::Capability::PermissionsRead),
        (c.permissions.can_write(), p::Capability::PermissionsWrite),
        (c.xattrs.can_read(), p::Capability::XattrsRead),
        (c.xattrs.can_write(), p::Capability::XattrsWrite),
    ] {
        if enabled {
            names.push(name.into());
        }
    }
    match c.read_file {
        FileReadSupport::None => (),
        FileReadSupport::Sequential => names.push(p::Capability::Read.into()),
        FileReadSupport::Range => names.push(p::Capability::RangeRead.into()),
    }
    match c.dir_model {
        DirModel::None => (),
        DirModel::Hierarchical => names.push(p::Capability::Hierarchical.into()),
        DirModel::Prefix => names.push(p::Capability::Prefix.into()),
    }
    names
}
pub fn decode_capabilities(names: &[i32]) -> Result<StorageCapabilities, DriverError> {
    let set: BTreeSet<_> = names.iter().copied().collect();
    if set.len() != names.len()
        || (set.contains(&(p::Capability::Prefix as i32))
            && set.contains(&(p::Capability::Hierarchical as i32)))
        || (set.contains(&(p::Capability::RangeRead as i32))
            && set.contains(&(p::Capability::Read as i32)))
        || (set.contains(&(p::Capability::PermissionsWrite as i32))
            && !set.contains(&(p::Capability::PermissionsRead as i32)))
        || (set.contains(&(p::Capability::XattrsWrite as i32))
            && !set.contains(&(p::Capability::XattrsRead as i32)))
    {
        return Err(invalid());
    }
    let attr = |r: p::Capability, w: p::Capability| {
        if set.contains(&(w as i32)) {
            AttributeSupport::ReadWrite
        } else if set.contains(&(r as i32)) {
            AttributeSupport::Read
        } else {
            AttributeSupport::None
        }
    };
    let capabilities = StorageCapabilities {
        stat_entry: set.contains(&(p::Capability::Stat as i32)),
        list_entries: set.contains(&(p::Capability::List as i32)),
        read_file: if set.contains(&(p::Capability::RangeRead as i32)) {
            FileReadSupport::Range
        } else if set.contains(&(p::Capability::Read as i32)) {
            FileReadSupport::Sequential
        } else {
            FileReadSupport::None
        },
        write_file: FileWriteSupport {
            create_new: set.contains(&(p::Capability::WriteNew as i32)),
            replace: set.contains(&(p::Capability::WriteReplace as i32)),
            atomic_publish: set.contains(&(p::Capability::AtomicPublish as i32)),
        },
        dir_model: if set.contains(&(p::Capability::Hierarchical as i32)) {
            DirModel::Hierarchical
        } else if set.contains(&(p::Capability::Prefix as i32)) {
            DirModel::Prefix
        } else {
            DirModel::None
        },
        persistent_empty_dirs: set.contains(&(p::Capability::EmptyDirs as i32)),
        create_dir: set.contains(&(p::Capability::CreateDir as i32)),
        remove_entry: set.contains(&(p::Capability::Remove as i32)),
        rename: RenameSupport {
            no_replace: set.contains(&(p::Capability::RenameNew as i32)),
            replace: set.contains(&(p::Capability::RenameReplace as i32)),
        },
        permissions: attr(
            p::Capability::PermissionsRead,
            p::Capability::PermissionsWrite,
        ),
        xattrs: attr(p::Capability::XattrsRead, p::Capability::XattrsWrite),
    };
    capabilities.validate().map_err(StorageError::from)?;
    Ok(capabilities)
}
pub fn describe(driver: &dyn Driver, generation: String) -> p::Description {
    let info = driver.info();
    p::Description {
        major: p::PROTOCOL_MAJOR,
        minor: p::PROTOCOL_MINOR,
        required_features: vec![],
        features: p::PROTOCOL_FEATURES
            .iter()
            .map(|feature| (*feature).into())
            .collect(),
        required_host_capabilities: vec![],
        instance_schema_profile: p::INSTANCE_SCHEMA_PROFILE.into(),
        generation,
        key: info.key().as_str().into(),
        name: info.name().clone(),
        version: info.version().to_string(),
        child_mounts: info.child_mount_policy() == ChildMountPolicy::Allow,
        capabilities: capabilities(info.capabilities()),

        settings_schema: info.settings_schema().cloned().map(Into::into),
        instance_schema: info.instance_config_schema().cloned().map(Into::into),
        config_version: info.instance_config_version().get(),
    }
}
pub fn driver_info(d: &p::Description) -> Result<DriverInfo, DriverError> {
    // Import availability is transport-specific; portable declarations are not.
    p::negotiate_protocol(d).map_err(|_| {
        DriverError::from(StorageError::new(StorageErrorKind::UnsupportedOperation))
    })?;
    if let Some(schema) = &d.instance_schema {
        ah_vfs::application::schema::validate_instance_config_schema(
            &p::json_object(schema.clone()).map_err(|_| invalid())?,
        )?;
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
        .maybe_settings_schema(
            d.settings_schema
                .clone()
                .map(p::json_object)
                .transpose()
                .map_err(|_| invalid())?,
        )
        .maybe_instance_config_schema(
            d.instance_schema
                .clone()
                .map(p::json_object)
                .transpose()
                .map_err(|_| invalid())?,
        )
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
            StorageEntryKind::File => p::EntryKind::File as i32,
            StorageEntryKind::Dir => p::EntryKind::Dir as i32,
            StorageEntryKind::Symlink => p::EntryKind::Symlink as i32,
            StorageEntryKind::Other => p::EntryKind::Other as i32,
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
    if e.kind != p::EntryKind::File as i32 && e.size.is_some() {
        return Err(storage_invalid());
    }
    Ok(StorageEntry {
        path: StoragePath::parse(&e.path).map_err(|_| storage_invalid())?,
        kind: match e.kind() {
            p::EntryKind::File => StorageEntryKind::File,
            p::EntryKind::Dir => StorageEntryKind::Dir,
            p::EntryKind::Symlink => StorageEntryKind::Symlink,
            p::EntryKind::Other => StorageEntryKind::Other,
            _ => return Err(storage_invalid()),
        },
        size: e.size,
        modified,
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

/// Decode required configuration objects at the VFS application boundary.
pub fn object(value: Option<p::Object>) -> Result<p::JsonObject, DriverError> {
    p::json_object(value.ok_or_else(invalid)?).map_err(|_| invalid())
}
