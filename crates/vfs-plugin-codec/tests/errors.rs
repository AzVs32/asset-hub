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
