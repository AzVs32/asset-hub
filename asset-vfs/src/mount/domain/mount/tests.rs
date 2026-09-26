use crate::driver::{DriverKind, DriverPath};
use crate::error::VfsError;
use crate::mount::error::MountError;
use crate::namespace::VirtualPath;

use super::{Mount, MountId};

fn mount_at(path: &str) -> Mount {
    Mount::new(
        MountId::new(),
        VirtualPath::try_from(path).unwrap(),
        DriverKind::try_from("test").unwrap(),
        DriverPath::new("driver-specific-root"),
        true,
    )
}

#[test]
fn mount_can_be_enabled_and_disabled() {
    let mut mount = mount_at("/assets");

    mount.disable();
    assert!(!mount.enabled());

    mount.enable();
    assert!(mount.enabled());
}

#[test]
fn mount_covers_its_mount_point_and_descendants() {
    let mount = mount_at("/a");

    assert!(mount.covers(&VirtualPath::try_from("/a").unwrap()));
    assert!(mount.covers(&VirtualPath::try_from("/a/b/file.txt").unwrap()));
    assert!(!mount.covers(&VirtualPath::try_from("/abc").unwrap()));
    assert!(!mount.covers(&VirtualPath::root()));
}

#[test]
fn mount_id_parses_and_round_trips() {
    let id = MountId::new();
    let parsed: MountId = id.to_string().parse().unwrap();

    assert_eq!(parsed, id);
}

#[test]
fn mount_id_parse_failure_returns_vfs_error() {
    let result: Result<MountId, VfsError> = "invalid-id".parse();

    assert!(matches!(
        result,
        Err(VfsError::Mount(MountError::InvalidId))
    ));
}
