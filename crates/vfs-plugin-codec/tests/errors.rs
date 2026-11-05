use ah_vfs::domain::{
    DriverError, InstanceConfigErrorKind, InstanceId,
    storage::{StorageError, StorageErrorKind},
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

    let error = DriverError::from(StorageError::with_source(
        StorageErrorKind::PermissionDenied,
        std::io::Error::other("private backend diagnostic"),
    ));
    let encoded = failure(error);
    assert_eq!(encoded.code(), p::FailureCode::PermissionDenied);
    assert!(encoded.field.is_none());
    assert!(encoded.config_kind.is_none());
    let wire = p::encode_reply(Err(encoded))?;
    let decoded = driver_error(
        p::decode_reply(&wire)?.err().ok_or("expected failure")?,
        None,
    );
    let storage = decoded.storage_error().ok_or("missing storage category")?;
    assert_eq!(storage.kind(), StorageErrorKind::PermissionDenied);
    assert!(storage.source().is_none());
    assert!(!storage.to_string().contains("private backend diagnostic"));
    Ok(())
}

#[test]
fn data_corruption_and_adapter_violations_remain_distinct_on_the_wire() -> Result {
    for kind in [
        StorageErrorKind::InvalidData,
        StorageErrorKind::ProtocolViolation,
    ] {
        let decoded = round_trip(
            StorageError::with_source(kind, std::io::Error::other("secret")).into(),
            None,
        )?;
        let storage = decoded.storage_error().ok_or("storage error")?;
        assert_eq!(storage.kind(), kind);
        assert!(!storage.to_string().contains("secret"));
        assert!(storage.source().is_none());
    }
    Ok(())
}

#[test]
fn response_paths_keep_their_validation_cause_instead_of_becoming_request_errors() -> Result {
    use ah_vfs::domain::storage::StoragePathError;
    let error = vfs_plugin_codec::decode_entry(p::Entry {
        path: "../private-location".into(),
        kind: p::EntryKind::File.into(),
        size: None,
        modified: None,
    })
    .err()
    .ok_or("invalid response path accepted")?;
    assert_eq!(error.kind(), StorageErrorKind::ProtocolViolation);
    assert!(
        error
            .source()
            .and_then(|e| e.downcast_ref::<StoragePathError>())
            .is_some()
    );
    assert!(!error.to_string().contains("private-location"));
    let error = vfs_plugin_codec::response_error(true, error);
    assert_eq!(error.kind(), StorageErrorKind::OutcomeUnknown);
    let violation = error
        .source()
        .and_then(|e| e.downcast_ref::<StorageError>())
        .ok_or("violation cause")?;
    assert_eq!(violation.kind(), StorageErrorKind::ProtocolViolation);
    assert!(
        violation
            .source()
            .and_then(|e| e.downcast_ref::<StoragePathError>())
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
            StorageErrorKind::InvalidRequest,
        ),
        (
            vfs_plugin_codec::response_object(Some(malformed)).err(),
            StorageErrorKind::ProtocolViolation,
        ),
    ] {
        let error = error.ok_or("invalid object accepted")?;
        let storage = error.storage_error().ok_or("storage error")?;
        assert_eq!(storage.kind(), kind);
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
    let storage = error.storage_error().ok_or("storage error")?;
    assert_eq!(storage.kind(), StorageErrorKind::ProtocolViolation);
    assert!(
        storage
            .source()
            .and_then(|e| e.downcast_ref::<InstanceIdError>())
            .is_some()
    );
    Ok(())
}
