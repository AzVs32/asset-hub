use std::error::Error;
use std::io;

use asset_core::domain::{DriverKind, DriverPath, EntryName, Mount, MountId, VirtualPath};
use asset_core::error::CoreError;
use asset_core::service::MountService;

#[test]
fn external_callers_match_component_errors_without_importing_payload_types() {
    let invalid_name: Result<EntryName, CoreError> = EntryName::try_from(".");
    assert!(matches!(invalid_name, Err(CoreError::Namespace(_))));
    let invalid_path: Result<VirtualPath, CoreError> = VirtualPath::try_from("relative");
    assert!(matches!(invalid_path, Err(CoreError::Namespace(_))));
    let invalid_kind: Result<DriverKind, CoreError> = DriverKind::try_from("");
    assert!(matches!(invalid_kind, Err(CoreError::DriverKind(_))));
    let invalid_id: Result<MountId, CoreError> = "invalid".parse();
    assert!(matches!(invalid_id, Err(CoreError::MountId(_))));

    let mount = Mount::new(
        MountId::new(),
        VirtualPath::root(),
        DriverKind::try_from("memory").unwrap(),
        DriverPath::new("/"),
        true,
    );
    let duplicate: Result<MountService, CoreError> = MountService::new(vec![mount.clone(), mount]);
    assert!(matches!(duplicate, Err(CoreError::MountService(_))));
}

#[test]
fn external_drivers_construct_and_classify_contract_errors() {
    let errors = [
        CoreError::driver_invalid_path(),
        CoreError::driver_not_found(),
        CoreError::driver_not_directory(),
        CoreError::driver_is_directory(),
        CoreError::driver_unrepresentable_name(),
    ];
    for (index, error) in errors.iter().enumerate() {
        assert!(matches!(error, CoreError::Driver(_)));
        let classifications = [
            error.is_invalid_driver_path(),
            error.is_not_found(),
            error.is_not_directory(),
            error.is_directory(),
            error.is_unrepresentable_name(),
        ];
        for (other_index, matches) in classifications.into_iter().enumerate() {
            assert_eq!(matches, index == other_index);
        }
    }
    let domain_error = VirtualPath::try_from("relative").unwrap_err();
    assert!(!domain_error.is_not_found());
}

#[test]
fn backend_failure_preserves_the_original_source_through_component_wrapping() {
    let error = CoreError::backend(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
    assert!(matches!(error, CoreError::Driver(_)));
    let component = error.source().unwrap();
    let original = component
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .unwrap();
    assert_eq!(original.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(original.to_string(), "denied");
    assert!(error.to_string().contains("denied"));
}
