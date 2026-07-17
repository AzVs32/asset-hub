use super::*;

#[test]
fn permission_capabilities_do_not_depend_on_enum_order() {
    assert!(DirectoryPermission::Full.allows(DirectoryPermission::Write));
    assert!(DirectoryPermission::Write.allows(DirectoryPermission::Read));
    assert!(!DirectoryPermission::Write.allows(DirectoryPermission::Full));
    assert!(!DirectoryPermission::Read.allows(DirectoryPermission::Write));
    assert_eq!(
        DirectoryPermission::Read.stronger(DirectoryPermission::Full),
        DirectoryPermission::Full
    );
}
