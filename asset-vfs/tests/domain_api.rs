use std::error::Error;

use asset_vfs::driver::{DriverError, DriverKind, DriverPath};
use asset_vfs::entry::{Entry, EntryKind};
use asset_vfs::error::VfsError;
use asset_vfs::mount::{Mount, MountError, MountId};
use asset_vfs::namespace::{EntryName, NamespaceError, VirtualPath, VirtualRelativePath};

#[test]
fn domains_work_together_through_their_public_entry_points() {
    let mount = Mount::new(
        MountId::new(),
        VirtualPath::try_from("/assets").unwrap(),
        DriverKind::try_from("memory").unwrap(),
        DriverPath::new("backend-root"),
        true,
    );
    let entry = Entry::file(EntryName::try_from("cover.jpg").unwrap(), Some(42));
    let path = mount.virtual_path().join_name(entry.name()).unwrap();
    let relative: VirtualRelativePath = path.strip_prefix(mount.virtual_path()).unwrap();

    assert!(mount.covers(&path));
    assert_eq!(relative.as_str(), "cover.jpg");
    assert_eq!(entry.kind(), EntryKind::File);
    assert_eq!(entry.size(), Some(42));
}

#[test]
fn fallible_domain_operations_return_vfs_errors_with_component_sources() {
    let invalid_path: Result<VirtualPath, VfsError> = VirtualPath::try_from("relative");
    let error = invalid_path.unwrap_err();
    assert!(matches!(
        error,
        VfsError::Namespace(NamespaceError::InvalidVirtualPath)
    ));
    assert_eq!(
        error.source().unwrap().to_string(),
        "virtual path must be absolute and use canonical `/` separators"
    );

    let invalid_name: Result<EntryName, VfsError> = EntryName::try_from(".");
    assert!(matches!(
        invalid_name,
        Err(VfsError::Namespace(NamespaceError::InvalidEntryName))
    ));
    let invalid_kind: Result<DriverKind, VfsError> = DriverKind::try_from("");
    let error = invalid_kind.unwrap_err();
    assert!(matches!(error, VfsError::Driver(DriverError::InvalidKind)));
    assert_eq!(
        error.source().unwrap().to_string(),
        "driver kind must be non-empty and contain only lowercase ASCII letters, digits, `.`, `-`, or `_`"
    );
    let invalid_id: Result<MountId, VfsError> = "invalid".parse();
    let error = invalid_id.unwrap_err();
    assert!(matches!(error, VfsError::Mount(MountError::InvalidId)));
    assert_eq!(
        error.source().unwrap().to_string(),
        "invalid mount identifier"
    );
}
