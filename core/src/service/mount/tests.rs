use crate::domain::DriverKind;
use crate::domain::{DriverPath, VirtualPath};
use crate::domain::{Mount, MountId};
use crate::error::CoreError;
use crate::service::internal::MountServiceError;

use super::{MountResolution, MountService};

fn mount_at(path: &str, enabled: bool) -> Mount {
    Mount::new(
        MountId::new(),
        VirtualPath::try_from(path).unwrap(),
        DriverKind::try_from("test").unwrap(),
        DriverPath::new("driver-specific-root"),
        enabled,
    )
}

fn path(value: &str) -> VirtualPath {
    VirtualPath::try_from(value).unwrap()
}

fn names(paths: Vec<VirtualPath>) -> Vec<String> {
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
    assert_eq!(deepest.mount().virtual_path().as_str(), "/a/b");
    assert_eq!(deepest.relative_path().as_str(), "file.txt");

    let MountResolution::Mounted(ancestor) = service.resolve(&path("/a/other.txt")) else {
        panic!("expected a mounted path");
    };
    assert_eq!(ancestor.mount().virtual_path().as_str(), "/a");
    assert_eq!(ancestor.relative_path().as_str(), "other.txt");
    assert_eq!(service.resolve(&path("/abc")), MountResolution::NotFound);
}

#[test]
fn resolve_returns_the_path_relative_to_the_mount_point() {
    let service = MountService::new(vec![mount_at("/movies", true)]).unwrap();

    let MountResolution::Mounted(resolved) = service.resolve(&path("/movies/2026/a.mp4")) else {
        panic!("expected a mounted path");
    };

    assert_eq!(resolved.mount().virtual_path().as_str(), "/movies");
    assert_eq!(resolved.relative_path().as_str(), "2026/a.mp4");
}

#[test]
fn resolving_a_mount_point_returns_an_empty_relative_path() {
    let service = MountService::new(vec![mount_at("/movies", true)]).unwrap();

    let MountResolution::Mounted(resolved) = service.resolve(&path("/movies")) else {
        panic!("expected a mounted path");
    };

    assert_eq!(resolved.relative_path().as_str(), "");
}

#[test]
fn resolving_through_the_root_mount_omits_the_leading_separator() {
    let service = MountService::new(vec![mount_at("/", true)]).unwrap();

    let MountResolution::Mounted(resolved) = service.resolve(&path("/movies/2026/a.mp4")) else {
        panic!("expected a mounted path");
    };

    assert_eq!(resolved.relative_path().as_str(), "movies/2026/a.mp4");
}

#[test]
fn exact_mount_points_remain_mounted_and_expose_nested_mounts_as_virtual_children() {
    let mount_a = mount_at("/movies", true);
    let mount_a_id = mount_a.id();
    let mount_b = mount_at("/movies/archive", true);
    let mount_b_id = mount_b.id();
    let service = MountService::new(vec![mount_a, mount_b]).unwrap();

    let MountResolution::Mounted(resolved_a) = service.resolve(&path("/movies")) else {
        panic!("expected mount A");
    };
    assert_eq!(resolved_a.mount().id(), mount_a_id);
    assert!(resolved_a.relative_path().is_empty());
    assert_eq!(
        names(service.virtual_children(&path("/movies"))),
        vec!["/movies/archive"]
    );

    let MountResolution::Mounted(resolved_b) = service.resolve(&path("/movies/archive")) else {
        panic!("expected mount B");
    };
    assert_eq!(resolved_b.mount().id(), mount_b_id);
    assert!(resolved_b.relative_path().is_empty());
}

#[test]
fn ancestors_of_nested_mounts_are_virtual_directories_over_the_covering_mount() {
    let mount_a = mount_at("/movies", true);
    let mount_a_id = mount_a.id();
    let mount_b = mount_at("/movies/archive/2026", true);
    let service = MountService::new(vec![mount_a, mount_b]).unwrap();

    let MountResolution::VirtualDirectory {
        underlying_mount: Some(underlying),
    } = service.resolve(&path("/movies/archive"))
    else {
        panic!("expected a virtual directory over mount A");
    };
    assert_eq!(underlying.mount().id(), mount_a_id);
    assert_eq!(underlying.relative_path().as_str(), "archive");
    assert_eq!(
        names(service.virtual_children(&path("/movies/archive"))),
        vec!["/movies/archive/2026"]
    );
}

#[test]
fn resolve_synthesizes_missing_ancestors() {
    let service = MountService::new(vec![mount_at("/c/b/a", true)]).unwrap();

    assert_eq!(
        service.resolve(&VirtualPath::root()),
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
    assert_eq!(
        names(service.virtual_children(&VirtualPath::root())),
        vec!["/c"]
    );
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
    assert_eq!(underlying.mount().virtual_path().as_str(), "/");
    assert_eq!(underlying.relative_path().as_str(), "c");
    assert_eq!(
        names(service.virtual_children(&VirtualPath::root())),
        vec!["/c"]
    );
    assert!(matches!(
        service.resolve(&VirtualPath::root()),
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

    assert_eq!(
        names(service.virtual_children(&VirtualPath::root())),
        vec!["/c"]
    );
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
    assert_eq!(mount.mount().virtual_path().as_str(), "/a");
    assert_eq!(mount.relative_path().as_str(), "b/file.txt");
    assert!(service.virtual_children(&path("/a")).is_empty());

    let disabled_only = MountService::new(vec![mount_at("/c/b/a", false)]).unwrap();
    assert_eq!(
        disabled_only.resolve(&path("/c")),
        MountResolution::NotFound
    );
    assert_eq!(
        disabled_only.resolve(&VirtualPath::root()),
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
        DriverKind::try_from("test").unwrap(),
        DriverPath::new("other-root"),
        true,
    );
    let duplicate_id = same_id.id();
    assert!(matches!(
        MountService::new(vec![first, same_id]).unwrap_err(),
        CoreError::MountService(MountServiceError::DuplicateMountId(value)) if value == duplicate_id
    ));

    let first = mount_at("/a", true);
    let same_path = mount_at("/a", false);
    assert!(matches!(
        MountService::new(vec![first, same_path]).unwrap_err(),
        CoreError::MountService(MountServiceError::DuplicateMountPath(value)) if value == path("/a")
    ));
}
