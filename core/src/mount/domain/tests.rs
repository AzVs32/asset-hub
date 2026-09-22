use crate::namespace::domain::{DriverPath, VirtualPath};

use super::{DriverKind, Mount, MountId, ResolvedMount};

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

#[test]
fn resolved_mount_derives_the_request_path_relative_to_the_mount() {
    let mount = mount_at("/movies");
    let request = VirtualPath::try_from("/movies/2026/a.mp4").unwrap();

    let resolved = ResolvedMount::new(&mount, &request).unwrap();

    assert_eq!(resolved.mount(), &mount);
    assert_eq!(resolved.relative_path().as_str(), "2026/a.mp4");
}

#[test]
fn resolved_mount_rejects_a_request_outside_the_mount() {
    let mount = mount_at("/movies");
    let request = VirtualPath::try_from("/music/a.flac").unwrap();

    assert_eq!(ResolvedMount::new(&mount, &request), None);
}
