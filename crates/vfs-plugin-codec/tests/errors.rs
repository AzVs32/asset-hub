use ah_vfs::application::driver_error::{DriverError, InstanceConfigErrorKind};
use ah_vfs::domain::InstanceId;
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
use std::error::Error;
use vfs_plugin_codec::{driver_error, failure};
use vfs_plugin_protocol as p;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn round_trip(error: DriverError, instance: Option<&str>) -> Result<DriverError> {
    let wire = p::encode_reply(Err(failure(error)))?;
    let failure = p::decode_reply(&wire)?
        .err()
        .ok_or("expected a wire failure")?;
    Ok(driver_error(failure, instance))
}

#[test]
fn known_config_error_categories_and_fields_survive_wire_round_trips() -> Result {
    for kind in [
        InstanceConfigErrorKind::MissingField,
        InstanceConfigErrorKind::UnknownField,
        InstanceConfigErrorKind::InvalidType,
        InstanceConfigErrorKind::InvalidValue,
    ] {
        let decoded = round_trip(
            DriverError::InvalidInstanceConfig {
                field: "root_path".into(),
                kind,
            },
            None,
        )?;
        assert!(matches!(decoded, DriverError::InvalidInstanceConfig {
            field, kind: actual,
        } if field == "root_path" && actual == kind));
    }
    Ok(())
}

#[test]
fn known_driver_failures_keep_their_categories_without_backend_diagnostics() -> Result {
    let id = InstanceId::generate();
    let decoded = round_trip(DriverError::InstanceNotFound(id.clone()), Some(id.as_str()))?;
    assert!(matches!(decoded, DriverError::InstanceNotFound(actual) if actual == id));

    let error = DriverError::from(StorageError::without_effects(
        Operation::Stat,
        Failure::Rejected(Rejection::AccessDenied),
        Some(Box::new(std::io::Error::other(
            "private backend diagnostic",
        ))),
    ));
    let encoded = failure(error);
    assert_eq!(encoded.code(), p::FailureCode::Storage);
    assert!(encoded.field.is_none());
    assert!(encoded.config_kind.is_none());
    let wire = p::encode_reply(Err(encoded))?;
    let decoded = driver_error(
        p::decode_reply(&wire)?.err().ok_or("expected failure")?,
        None,
    );
    let storage = decoded.storage_error().ok_or("missing storage category")?;
    assert_eq!(
        storage.failure(),
        Failure::Rejected(Rejection::AccessDenied)
    );
    assert!(storage.source().is_none());
    assert!(!storage.to_string().contains("private backend diagnostic"));
    Ok(())
}

#[test]
fn data_corruption_and_adapter_violations_remain_distinct_on_the_wire() -> Result {
    for kind in [
        Failure::Backend(BackendFailure::CorruptData),
        Failure::Contract(ContractViolation::InvalidResponse),
    ] {
        let decoded = round_trip(
            StorageError::without_effects(
                Operation::Stat,
                kind,
                Some(Box::new(std::io::Error::other("secret"))),
            )
            .into(),
            None,
        )?;
        let storage = decoded.storage_error().ok_or("storage error")?;
        assert_eq!(storage.failure(), kind);
        assert!(!storage.to_string().contains("secret"));
        assert!(storage.source().is_none());
    }
    Ok(())
}

#[test]
fn response_paths_keep_their_validation_cause_instead_of_becoming_request_errors() -> Result {
    use ah_vfs_storage::PathError;
    let error = vfs_plugin_codec::decode_entry(
        Operation::Write,
        p::Entry {
            path: "../private-location".into(),
            kind: p::EntryKind::File.into(),
            size: None,
            modified: None,
        },
    )
    .err()
    .ok_or("invalid response path accepted")?;
    assert_eq!(
        error.failure(),
        Failure::Contract(ContractViolation::InvalidResponse)
    );
    assert!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<PathError>())
            .is_some()
    );
    assert!(!error.to_string().contains("private-location"));
    let error = vfs_plugin_codec::response_error(Operation::Write, true, error);
    assert_eq!(
        error.failure(),
        Failure::Contract(ContractViolation::InvalidResponse)
    );
    assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unknown);
    let violation = error
        .source()
        .and_then(|e| e.downcast_ref::<StorageError>())
        .ok_or("violation cause")?;
    assert_eq!(
        violation.failure(),
        Failure::Contract(ContractViolation::InvalidResponse)
    );
    assert!(
        violation
            .source()
            .and_then(|e| e.downcast_ref::<PathError>())
            .is_some()
    );
    Ok(())
}

#[test]
fn configuration_objects_are_classified_by_direction_and_keep_the_codec_cause() -> Result {
    let malformed = p::Object {
        fields: [("field".into(), p::JsonValue { kind: None })].into(),
    };
    for (error, kind) in [
        (
            vfs_plugin_codec::request_object(Some(malformed.clone())).err(),
            ah_vfs::application::driver_error::LifecycleFailure::InvalidRequest,
        ),
        (
            vfs_plugin_codec::response_object(Some(malformed)).err(),
            ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration,
        ),
    ] {
        let error = error.ok_or("invalid object accepted")?;
        let storage = error.lifecycle_error().ok_or("storage error")?;
        assert_eq!(storage.failure(), kind);
        assert!(
            storage
                .source()
                .and_then(|e| e.downcast_ref::<p::CodecError>())
                .is_some()
        );
    }
    Ok(())
}

#[test]
fn malformed_instance_failure_keeps_the_id_validation_cause() -> Result {
    use ah_vfs::domain::InstanceIdError;
    let error = driver_error(
        p::Failure::new(p::FailureCode::InstanceNotFound),
        Some("bad id"),
    );
    let storage = error.lifecycle_error().ok_or("storage error")?;
    assert_eq!(
        storage.failure(),
        ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration
    );
    assert!(
        storage
            .source()
            .and_then(|e| e.downcast_ref::<InstanceIdError>())
            .is_some()
    );
    Ok(())
}

#[test]
fn dispatch_evidence_round_trips_independently_of_the_failure_cause() -> Result {
    use ah_vfs_storage::MutationOutcome;
    for operation in [
        Operation::Read,
        Operation::Write,
        Operation::CreateDirectory,
        Operation::Rename,
        Operation::Remove,
        Operation::SetPermissions,
        Operation::SetXattr,
        Operation::RemoveXattr,
    ] {
        for cause in [
            Failure::Backend(BackendFailure::TimedOut),
            Failure::Rejected(Rejection::AccessDenied),
            Failure::Contract(ContractViolation::InvalidEntry),
        ] {
            for dispatched in [false, true] {
                let error = if dispatched {
                    StorageError::after_dispatch(operation, cause, None)
                } else {
                    StorageError::without_effects(operation, cause, None)
                };
                let decoded = round_trip(error.into(), None)?;
                let storage = decoded.storage_error().ok_or("storage failure")?;
                assert_eq!(storage.operation(), operation);
                assert_eq!(storage.failure(), cause);
                assert_eq!(
                    storage.outcome(),
                    if dispatched && operation.may_mutate() {
                        MutationOutcome::Unknown
                    } else {
                        MutationOutcome::Unchanged
                    }
                );
            }
        }
    }
    Ok(())
}
#[test]
fn unknown_failure_tags_and_read_mutation_outcomes_are_rejected() -> Result {
    let error = StorageError::without_effects(
        Operation::Read,
        Failure::Backend(BackendFailure::TimedOut),
        None,
    );
    let encoded = failure(error.into());
    for mutate in [0, 1, 2, 3] {
        let mut wire = encoded.clone();
        let storage = wire.storage.as_mut().ok_or("storage")?;
        match mutate {
            0 => storage.operation = "unknown".into(),
            1 => storage.reason = "unknown".into(),
            2 => storage.family = "unknown".into(),
            _ => storage.outcome = p::MutationOutcome::Unknown.into(),
        }
        let error = driver_error(wire, None);
        assert!(error.storage_error().is_none());
        assert_eq!(
            error.lifecycle_error().ok_or("protocol failure")?.failure(),
            ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration
        );
    }
    Ok(())
}
