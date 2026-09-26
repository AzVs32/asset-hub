use crate::driver::{DriverKind, DriverPath};
use crate::mount::{Mount, MountId};
use crate::namespace::VirtualPath;

use super::ResolvedMount;

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
