use crate::namespace::domain::{DriverPath, VirtualPath};

use super::{DriverKind, Mount, MountId};

fn mount_at(path: &str) -> Mount {
    Mount::new(
        MountId::new(),
        VirtualPath::try_from(path).unwrap(),
        DriverKind,
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
