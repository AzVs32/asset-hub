use super::DriverKind;
use crate::driver::error::DriverError;
use crate::error::VfsError;

#[test]
fn driver_kind_is_a_stable_registry_identifier() {
    let kind = DriverKind::try_from("memory.v1").unwrap();

    assert_eq!(kind.as_str(), "memory.v1");
    assert_eq!(kind.as_ref(), "memory.v1");
    assert_eq!(kind.to_string(), "memory.v1");
    assert!(matches!(
        DriverKind::try_from("Memory").unwrap_err(),
        VfsError::Driver(DriverError::InvalidKind)
    ));
}

#[test]
fn driver_kind_rejects_empty_and_oversized_identifiers() {
    assert!(matches!(
        DriverKind::try_from("").unwrap_err(),
        VfsError::Driver(DriverError::InvalidKind)
    ));

    let oversized = "a".repeat(65);
    assert!(matches!(
        DriverKind::try_from(oversized.as_str()).unwrap_err(),
        VfsError::Driver(DriverError::KindTooLong { length, max }) if length == 65 && max == 64
    ));
}
