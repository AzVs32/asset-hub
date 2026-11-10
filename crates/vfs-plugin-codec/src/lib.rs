//! Checked conversion between portable values and shared VFS domain types.
use ah_vfs::application::driver_error::{
    DriverError, InstanceConfigErrorKind, LifecycleError, LifecycleFailure,
};
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::port::*;
use ah_vfs::domain::namespace::ChildMountPolicy;
use ah_vfs::domain::{DriverKey, InstanceId};
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use ah_vfs_storage::{Capabilities, Entry};
use std::num::NonZeroU32;
use vfs_plugin_protocol as p;

/// A directory binding can expire while the storage session remains live.
pub fn is_stale_directory(error: &StorageError) -> bool {
    error.operation() == Operation::List
        && error.failure() == Failure::Backend(BackendFailure::Stale)
}

pub fn protocol_violation() -> DriverError {
    LifecycleError::new(LifecycleFailure::InvalidDeclaration).into()
}
pub fn protocol_violation_with_source(
    source: impl std::error::Error + Send + Sync + 'static,
) -> DriverError {
    LifecycleError::with_source(LifecycleFailure::InvalidDeclaration, source).into()
}
pub fn storage_protocol_violation(operation: Operation) -> StorageError {
    StorageError::after_dispatch(
        operation,
        Failure::Contract(ContractViolation::InvalidResponse),
        None,
    )
}
pub fn storage_protocol_violation_with_source(
    operation: Operation,
    source: impl std::error::Error + Send + Sync + 'static,
) -> StorageError {
    StorageError::after_dispatch(
        operation,
        Failure::Contract(ContractViolation::InvalidResponse),
        Some(Box::new(source)),
    )
}
/// Preserve the cause independently from dispatch and publication evidence.
pub fn response_error(
    operation: Operation,
    may_publish: bool,
    source: StorageError,
) -> StorageError {
    let failure = source.failure();
    if may_publish {
        StorageError::after_dispatch(operation, failure, Some(Box::new(source)))
    } else {
        StorageError::without_effects(operation, failure, Some(Box::new(source)))
    }
}
/// Attach the real request context to local transport/validation failures.
/// Control-plane failures never receive a fabricated storage operation.
pub fn call_error(
    op: &p::Operation,
    failure: Failure,
    dispatched: bool,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
) -> DriverError {
    if let Some(operation) = op.storage_operation() {
        return if dispatched && op.may_publish() {
            StorageError::after_dispatch(operation, failure, source)
        } else {
            StorageError::without_effects(operation, failure, source)
        }
        .into();
    }
    let category = match failure {
        Failure::Backend(BackendFailure::TimedOut) => LifecycleFailure::TimedOut,
        Failure::Backend(BackendFailure::Unavailable) => LifecycleFailure::Unavailable,
        Failure::Backend(BackendFailure::Stale | BackendFailure::Closed) => LifecycleFailure::Stale,
        Failure::Backend(BackendFailure::Cancelled) => LifecycleFailure::Cancelled,
        Failure::Backend(BackendFailure::CapacityExceeded)
        | Failure::Rejected(Rejection::LimitExceeded) => LifecycleFailure::CapacityExceeded,
        Failure::Rejected(Rejection::AccessDenied) => LifecycleFailure::AccessDenied,
        Failure::Rejected(Rejection::Unsupported) => LifecycleFailure::Unsupported,
        Failure::Rejected(Rejection::Conflict) => LifecycleFailure::Conflict,
        Failure::Contract(_) => LifecycleFailure::InvalidDeclaration,
        _ => LifecycleFailure::Backend,
    };
    match source {
        Some(source) => LifecycleError::with_boxed_source(category, source).into(),
        None => LifecycleError::new(category).into(),
    }
}
const LIFECYCLE_FAILURES: &[(LifecycleFailure, p::FailureCode)] = {
    use LifecycleFailure as L;
    use p::FailureCode as W;
    &[
        (L::InvalidRecord, W::LifecycleInvalidRecord),
        (L::InvalidRequest, W::LifecycleInvalidRequest),
        (L::InvalidDeclaration, W::LifecycleInvalidDeclaration),
        (L::Unsupported, W::LifecycleUnsupported),
        (L::Conflict, W::LifecycleConflict),
        (L::Unavailable, W::LifecycleUnavailable),
        (L::TimedOut, W::LifecycleTimedOut),
        (L::Backend, W::LifecycleBackend),
        (L::AccessDenied, W::LifecycleAccessDenied),
        (L::Stale, W::LifecycleStale),
        (L::CapacityExceeded, W::LifecycleCapacityExceeded),
        (L::Cancelled, W::LifecycleCancelled),
    ]
};

pub fn failure(error: DriverError) -> p::Failure {
    match error {
        DriverError::Storage(error) => match p::encode_storage_error(&error) {
            Ok(storage) => p::Failure {
                storage: Some(storage),
                ..p::Failure::new(p::FailureCode::Storage)
            },
            Err(_) => p::Failure::new(p::FailureCode::ProtocolViolation),
        },
        DriverError::Lifecycle(error) => {
            let code = LIFECYCLE_FAILURES
                .iter()
                .find(|(failure, _)| *failure == error.failure())
                .map(|(_, code)| *code)
                .unwrap_or(p::FailureCode::LifecycleBackend);
            p::Failure::new(code)
        }
        DriverError::InstanceNotFound(_) => p::Failure::new(p::FailureCode::InstanceNotFound),
        DriverError::InvalidInstanceConfig { field, kind } => p::Failure {
            code: p::FailureCode::InvalidInstanceConfig as i32,
            storage: None,
            field: Some(field),
            config_kind: Some(
                match kind {
                    InstanceConfigErrorKind::MissingField => p::ConfigErrorKind::MissingField,
                    InstanceConfigErrorKind::UnknownField => p::ConfigErrorKind::UnknownField,
                    InstanceConfigErrorKind::InvalidType => p::ConfigErrorKind::InvalidType,
                    InstanceConfigErrorKind::InvalidValue => p::ConfigErrorKind::InvalidValue,
                    // Older protocol adapters cannot represent a newer domain category.
                    _ => return p::Failure::new(p::FailureCode::LifecycleBackend),
                }
                .into(),
            ),
        },
        // Future driver failures retain a conservative, diagnostic-free wire category.
        _ => p::Failure::new(p::FailureCode::LifecycleBackend),
    }
}
pub fn driver_error(error: p::Failure, instance: Option<&str>) -> DriverError {
    let Ok(code) = p::FailureCode::try_from(error.code) else {
        return protocol_violation();
    };
    if code == p::FailureCode::Storage {
        if error.field.is_some() || error.config_kind.is_some() {
            return protocol_violation();
        }
        return match error.storage {
            Some(storage) => p::decode_storage_error(storage)
                .map(DriverError::from)
                .unwrap_or_else(protocol_violation_with_source),
            None => protocol_violation(),
        };
    }
    if error.storage.is_some() {
        return protocol_violation();
    }
    if let Some((failure, _)) = LIFECYCLE_FAILURES
        .iter()
        .find(|(_, expected)| *expected == code)
    {
        if error.field.is_some() || error.config_kind.is_some() {
            return LifecycleError::new(LifecycleFailure::InvalidDeclaration).into();
        }
        return LifecycleError::new(*failure).into();
    }
    match code {
        p::FailureCode::InstanceNotFound => match instance.map(InstanceId::parse) {
            Some(Ok(id)) => DriverError::InstanceNotFound(id),
            Some(Err(source)) => protocol_violation_with_source(source),
            None => protocol_violation(),
        },
        p::FailureCode::InvalidInstanceConfig => {
            let Some(raw_kind) = error.config_kind else {
                return protocol_violation();
            };
            let kind = match p::ConfigErrorKind::try_from(raw_kind) {
                Ok(p::ConfigErrorKind::MissingField) => InstanceConfigErrorKind::MissingField,
                Ok(p::ConfigErrorKind::UnknownField) => InstanceConfigErrorKind::UnknownField,
                Ok(p::ConfigErrorKind::InvalidType) => InstanceConfigErrorKind::InvalidType,
                Ok(p::ConfigErrorKind::InvalidValue) => InstanceConfigErrorKind::InvalidValue,
                Ok(_) => return protocol_violation(),
                Err(source) => return protocol_violation_with_source(source),
            };
            let Some(field) = error.field else {
                return protocol_violation();
            };
            DriverError::InvalidInstanceConfig { field, kind }
        }
        p::FailureCode::InvalidRequest => {
            LifecycleError::new(LifecycleFailure::InvalidRequest).into()
        }
        p::FailureCode::TooLarge => LifecycleError::new(LifecycleFailure::CapacityExceeded).into(),
        _ => protocol_violation(),
    }
}
pub fn decode_capabilities(names: &[i32]) -> Result<Capabilities, DriverError> {
    p::decode_capabilities(names).map_err(|cause| {
        LifecycleError::with_source(LifecycleFailure::InvalidDeclaration, cause).into()
    })
}
pub fn describe(driver: &dyn Driver, generation: String) -> p::Description {
    let info = driver.info();
    p::Description {
        key: info.key().as_str().into(),
        name: info.name().clone(),
        version: info.version().to_string(),
        child_mounts: info.child_mount_policy() == ChildMountPolicy::Allow,
        capabilities: p::encode_capabilities(info.capabilities()),

        settings_schema: info.settings_schema().cloned().map(Into::into),
        instance_schema: info.instance_config_schema().cloned().map(Into::into),
        config_version: info.instance_config_version().get(),
        ..p::description_header(generation)
    }
}
pub fn driver_info(d: &p::Description) -> Result<DriverInfo, DriverError> {
    // Import availability is transport-specific; portable declarations are not.
    p::negotiate_protocol(d)
        .map_err(|error| LifecycleError::with_source(LifecycleFailure::Unsupported, error))?;
    if let Some(schema) = &d.instance_schema {
        ah_vfs::application::schema::validate_instance_config_schema(
            &p::json_object(schema.clone()).map_err(protocol_violation_with_source)?,
        )?;
    }
    if d.generation.is_empty() || d.generation.len() > 128 || d.name.len() > 4096 {
        return Err(protocol_violation());
    }
    Ok(DriverInfo::builder()
        .key(DriverKey::parse(&d.key).map_err(protocol_violation_with_source)?)
        .name(d.name.clone())
        .version(semver::Version::parse(&d.version).map_err(protocol_violation_with_source)?)
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
                .map_err(protocol_violation_with_source)?,
        )
        .maybe_instance_config_schema(
            d.instance_schema
                .clone()
                .map(p::json_object)
                .transpose()
                .map_err(protocol_violation_with_source)?,
        )
        .instance_config_version(NonZeroU32::new(d.config_version).ok_or_else(protocol_violation)?)
        .build())
}
pub fn entry(operation: Operation, entry: Entry) -> Result<p::Entry, StorageError> {
    p::encode_entry(&entry).map_err(|cause| invalid_entry(operation, cause))
}

pub fn decode_entry(operation: Operation, entry: p::Entry) -> Result<Entry, StorageError> {
    p::decode_entry(entry).map_err(|cause| invalid_entry(operation, cause))
}

fn invalid_entry(operation: Operation, cause: p::EntryValueError) -> StorageError {
    let source: Box<dyn std::error::Error + Send + Sync> = match cause {
        p::EntryValueError::Path(cause) => Box::new(cause),
        cause => Box::new(cause),
    };
    StorageError::after_dispatch(
        operation,
        Failure::Contract(ContractViolation::InvalidEntry),
        Some(source),
    )
}

/// An unexpected control-plane failure during storage execution is a contract bug.
/// Valid storage failures retain their independent cause and outcome.
pub fn storage_error(operation: Operation, may_publish: bool, error: DriverError) -> StorageError {
    match error {
        DriverError::Storage(error) if error.operation() == operation => error,
        error => response_error(
            operation,
            may_publish,
            storage_protocol_violation_with_source(operation, error),
        ),
    }
}

/// Decode required request configuration objects, preserving validation causes.
pub fn request_object(value: Option<p::Object>) -> Result<p::JsonObject, DriverError> {
    let invalid = || LifecycleError::new(LifecycleFailure::InvalidRequest);
    p::json_object(value.ok_or_else(invalid)?).map_err(|source| {
        LifecycleError::with_source(LifecycleFailure::InvalidRequest, source).into()
    })
}
/// Decode configuration objects returned by an adapter.
pub fn response_object(value: Option<p::Object>) -> Result<p::JsonObject, DriverError> {
    p::json_object(value.ok_or_else(protocol_violation)?).map_err(protocol_violation_with_source)
}
