use crate::mount::domain::{DriverKind, Mount, MountId};
use crate::mount::error::ServiceError;
use crate::path::domain::{DPath, VPath};

use super::{MountResolution, MountService};

fn mount_at(path: &str, enabled: bool) -> Mount {
    Mount::new(
        MountId::new(),
        VPath::parse(path).unwrap(),
        DriverKind,
        DPath::new("driver-specific-root"),
        enabled,
    )
}

fn path(value: &str) -> VPath {
    VPath::parse(value).unwrap()
}

fn names(paths: Vec<VPath>) -> Vec<String> {
    paths
        .into_iter()
        .map(|path| path.as_str().to_owned())
        .collect()
}

#[test]
fn resolve_prefers_the_deepest_enabled_mount() {
    let service = MountService::new(vec![mount_at("/a", true), mount_at("/a/b", true)]).unwrap();

    let MountResolution::Mounted(deepest) = service.resolve(&path("/a/b/file.txt")) else {
        panic!("expected a mounted path");
    };
    assert_eq!(deepest.v_path().as_str(), "/a/b");

    let MountResolution::Mounted(ancestor) = service.resolve(&path("/a/other.txt")) else {
        panic!("expected a mounted path");
    };
    assert_eq!(ancestor.v_path().as_str(), "/a");
    assert_eq!(service.resolve(&path("/abc")), MountResolution::NotFound);
}

#[test]
fn resolve_synthesizes_missing_ancestors() {
    let service = MountService::new(vec![mount_at("/c/b/a", true)]).unwrap();

    assert_eq!(
        service.resolve(&VPath::root()),
        MountResolution::VirtualDirectory {
            underlying_mount: None
        }
    );
    assert_eq!(
        service.resolve(&path("/c/b")),
        MountResolution::VirtualDirectory {
            underlying_mount: None
        }
    );
    assert!(matches!(
        service.resolve(&path("/c/b/a")),
        MountResolution::Mounted(_)
    ));
    assert_eq!(names(service.virtual_children(&VPath::root())), vec!["/c"]);
    assert_eq!(names(service.virtual_children(&path("/c"))), vec!["/c/b"]);
    assert_eq!(
        names(service.virtual_children(&path("/c/b"))),
        vec!["/c/b/a"]
    );
}

#[test]
fn virtual_directories_retain_the_underlying_mount() {
    let service = MountService::new(vec![mount_at("/", true), mount_at("/c/b/a", true)]).unwrap();

    let MountResolution::VirtualDirectory {
        underlying_mount: Some(underlying),
    } = service.resolve(&path("/c"))
    else {
        panic!("expected a virtual directory over a mounted path");
    };
    assert_eq!(underlying.v_path().as_str(), "/");
    assert_eq!(names(service.virtual_children(&VPath::root())), vec!["/c"]);
    assert!(matches!(
        service.resolve(&VPath::root()),
        MountResolution::Mounted(_)
    ));
}

#[test]
fn virtual_children_are_unique_and_sorted() {
    let service = MountService::new(vec![
        mount_at("/c/d", true),
        mount_at("/c/b/a", true),
        mount_at("/c/b/z", true),
    ])
    .unwrap();

    assert_eq!(names(service.virtual_children(&VPath::root())), vec!["/c"]);
    assert_eq!(
        names(service.virtual_children(&path("/c"))),
        vec!["/c/b", "/c/d"]
    );
}

#[test]
fn disabled_mounts_do_not_participate_in_resolution() {
    let service = MountService::new(vec![mount_at("/a", true), mount_at("/a/b", false)]).unwrap();

    let MountResolution::Mounted(mount) = service.resolve(&path("/a/b/file.txt")) else {
        panic!("expected the enabled ancestor mount");
    };
    assert_eq!(mount.v_path().as_str(), "/a");
    assert!(service.virtual_children(&path("/a")).is_empty());

    let disabled_only = MountService::new(vec![mount_at("/c/b/a", false)]).unwrap();
    assert_eq!(
        disabled_only.resolve(&path("/c")),
        MountResolution::NotFound
    );
    assert_eq!(
        disabled_only.resolve(&VPath::root()),
        MountResolution::VirtualDirectory {
            underlying_mount: None
        }
    );
}

#[test]
fn duplicate_mount_identity_and_path_are_rejected() {
    let first = mount_at("/a", true);
    let same_id = Mount::new(
        first.id(),
        path("/b"),
        DriverKind,
        DPath::new("other-root"),
        true,
    );
    let duplicate_id = same_id.id();
    assert_eq!(
        MountService::new(vec![first, same_id]).unwrap_err(),
        ServiceError::DuplicateMountId(duplicate_id)
    );

    let first = mount_at("/a", true);
    let same_path = mount_at("/a", false);
    assert_eq!(
        MountService::new(vec![first, same_path]).unwrap_err(),
        ServiceError::DuplicateMountPath(path("/a"))
    );
}
