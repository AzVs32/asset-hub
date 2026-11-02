use ah_vfs::domain::{
    ChildMountPolicy, DriverInstanceRef, DriverKey, InstanceId, Mount, MountId, MountTable,
    MountTableError, VirtualPath,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn mount(path: &str, driver: &str, enabled: bool) -> Result<Mount, Box<dyn std::error::Error>> {
    Ok(Mount::builder()
        .id(MountId::new())
        .virtual_path(VirtualPath::parse(path)?)
        .driver_instance(DriverInstanceRef::new(
            DriverKey::parse(driver)?,
            InstanceId::parse("42")?,
        ))
        .enabled(enabled)
        .build())
}

fn policy(driver: &DriverKey) -> Option<ChildMountPolicy> {
    match driver.as_str() {
        "builtin.local" => Some(ChildMountPolicy::Allow),
        "acme.s3" => Some(ChildMountPolicy::Deny),
        _ => None,
    }
}

#[test]
fn root_is_required_and_cannot_be_removed() -> TestResult {
    assert!(matches!(
        MountTable::new(mount("/data", "builtin.local", true)?),
        Err(MountTableError::RootRequired)
    ));

    let root = mount("////", "builtin.local", true)?;
    let id = root.id();
    let mut table = MountTable::new(root)?;
    assert_eq!(table.root().id(), id);
    assert!(matches!(
        table.remove(id),
        Err(MountTableError::RootCannotBeRemoved)
    ));
    assert_eq!(table.list().len(), 1);
    assert_eq!(table.resolve(&VirtualPath::root())?.mount().id(), id);
    assert_eq!(table.resolve(&VirtualPath::root())?.relative_path(), "");
    Ok(())
}

#[test]
fn duplicate_ids_and_canonical_paths_leave_the_table_unchanged() -> TestResult {
    let root = mount("/", "builtin.local", true)?;
    let root_id = root.id();
    let mut table = MountTable::new(root)?;
    let duplicate_id = Mount::builder()
        .id(root_id)
        .virtual_path(VirtualPath::parse("/another")?)
        .driver_instance(DriverInstanceRef::new(
            DriverKey::parse("builtin.local")?,
            InstanceId::parse("43")?,
        ))
        .enabled(true)
        .build();
    assert!(
        matches!(table.insert(duplicate_id, policy), Err(MountTableError::DuplicateId(id)) if id == root_id)
    );
    assert!(
        matches!(table.insert(mount("/", "acme.s3", true)?, policy), Err(MountTableError::PathOccupied(path)) if path.is_root())
    );

    let original = mount("//文档///保存/", "builtin.local", false)?;
    let original_id = original.id();
    table.insert(original, policy)?;
    assert!(
        matches!(table.insert(mount("/文档/保存", "acme.s3", true)?, policy), Err(MountTableError::PathOccupied(path)) if path.as_str() == "/文档/保存")
    );
    assert_eq!(table.list().len(), 2);
    assert_eq!(
        table
            .get_by_path(&VirtualPath::parse("/文档/保存")?)
            .map(Mount::id),
        Some(original_id)
    );
    assert_eq!(table.get(original_id).map(Mount::enabled), Some(false));
    Ok(())
}

#[test]
fn parent_driver_controls_nesting_even_when_disabled() -> TestResult {
    let mut table = MountTable::new(mount("/", "builtin.local", true)?)?;
    table.insert(mount("/local", "builtin.local", true)?, policy)?;
    table.insert(mount("/local/archive", "acme.s3", false)?, policy)?;

    for path in ["/local/archive/child", "/local/archive/deeper/child"] {
        assert!(matches!(
            table.insert(mount(path, "builtin.local", true)?, policy),
            Err(MountTableError::ChildMountsForbidden { driver, path })
                if driver.as_str() == "acme.s3" && path.as_str() == "/local/archive"
        ));
    }
    table.insert(mount("/local/archive-other", "acme.s3", true)?, policy)?;
    assert_eq!(table.list().len(), 4);

    let mut s3_root = MountTable::new(mount("/", "acme.s3", true)?)?;
    assert!(
        matches!(s3_root.insert(mount("/child", "builtin.local", true)?, policy), Err(MountTableError::ChildMountsForbidden { path, .. }) if path.is_root())
    );
    assert_eq!(s3_root.list().len(), 1);
    Ok(())
}

#[test]
fn inserting_a_parent_cannot_bypass_existing_child_constraints() -> TestResult {
    let mut table = MountTable::new(mount("/", "builtin.local", true)?)?;
    let child = mount("/data/deep/child", "acme.s3", false)?;
    let child_id = child.id();
    table.insert(child, policy)?;
    for enabled in [true, false] {
        assert!(matches!(
            table.insert(mount("/data", "acme.s3", enabled)?, policy),
            Err(MountTableError::ChildMountsForbidden { driver, path })
                if driver.as_str() == "acme.s3" && path.as_str() == "/data"
        ));
        assert_eq!(table.list().len(), 2);
    }
    table.insert(mount("/data", "builtin.local", true)?, policy)?;
    assert_eq!(table.list().len(), 3);
    assert_eq!(table.get(child_id).map(Mount::enabled), Some(false));
    Ok(())
}

#[test]
fn missing_policies_fail_before_mutation() -> TestResult {
    let mut table = MountTable::new(mount("/", "unavailable.driver", true)?)?;
    assert!(
        matches!(table.insert(mount("/data", "builtin.local", true)?, policy), Err(MountTableError::PolicyUnavailable(key)) if key.as_str() == "unavailable.driver")
    );
    assert_eq!(table.list().len(), 1);

    let mut table = MountTable::new(mount("/", "builtin.local", true)?)?;
    table.insert(mount("/data/child", "acme.s3", true)?, policy)?;
    assert!(
        matches!(table.insert(mount("/data", "unavailable.driver", true)?, policy), Err(MountTableError::PolicyUnavailable(key)) if key.as_str() == "unavailable.driver")
    );
    assert_eq!(table.list().len(), 2);
    Ok(())
}

#[test]
fn resolution_selects_the_deepest_whole_segment_match() -> TestResult {
    let root = mount("/", "builtin.local", true)?;
    let root_id = root.id();
    let mut table = MountTable::new(root)?;
    let deep = mount("/docs/reports", "acme.s3", true)?;
    let deep_id = deep.id();
    table.insert(deep, policy)?;
    let docs = mount("/docs", "builtin.local", true)?;
    let docs_id = docs.id();
    table.insert(docs, policy)?;
    let unicode = mount("/文档", "acme.s3", true)?;
    let unicode_id = unicode.id();
    table.insert(unicode, policy)?;

    for (input, expected_id, expected_suffix) in [
        ("/", root_id, ""),
        ("/other/file", root_id, "other/file"),
        ("/Docs/file", root_id, "Docs/file"),
        ("/docs-old/file", root_id, "docs-old/file"),
        ("/docs", docs_id, ""),
        ("/docs/file", docs_id, "file"),
        ("/docs/reports-old/file", docs_id, "reports-old/file"),
        ("/docs/reports", deep_id, ""),
        ("//docs///reports/a.txt/", deep_id, "a.txt"),
        ("/文档/😀", unicode_id, "😀"),
    ] {
        let path = VirtualPath::parse(input)?;
        let resolved = table.resolve(&path)?;
        assert_eq!(resolved.mount().id(), expected_id, "path={input}");
        assert_eq!(resolved.relative_path(), expected_suffix, "path={input}");
    }
    assert!(
        table
            .get_by_path(&VirtualPath::parse("/docs/file")?)
            .is_none()
    );
    assert_eq!(
        table
            .list()
            .iter()
            .map(|mount| mount.virtual_path().as_str())
            .collect::<Vec<_>>(),
        ["/", "/docs", "/docs/reports", "/文档"]
    );
    Ok(())
}

#[test]
fn disabled_boundaries_do_not_fall_back_but_deeper_mounts_remain_independent() -> TestResult {
    let mut table = MountTable::new(mount("/", "builtin.local", true)?)?;
    let root_id = table.root().id();
    let docs = mount("/docs", "builtin.local", true)?;
    let docs_id = docs.id();
    let instance = docs.driver_instance().clone();
    table.insert(docs, policy)?;
    let nested = mount("/docs/archive", "acme.s3", true)?;
    let nested_id = nested.id();
    table.insert(nested, policy)?;
    table.disable(docs_id)?;
    table.disable(docs_id)?;

    for input in ["/docs", "/docs/file", "/docs/archive-other/file"] {
        assert!(
            matches!(table.resolve(&VirtualPath::parse(input)?), Err(MountTableError::Disabled(id)) if id == docs_id)
        );
    }
    assert_eq!(
        table.get(docs_id).map(Mount::driver_instance),
        Some(&instance)
    );
    assert_eq!(
        table
            .resolve(&VirtualPath::parse("/docs/archive/file")?)?
            .mount()
            .id(),
        nested_id
    );
    assert_eq!(
        table
            .resolve(&VirtualPath::parse("/docs-other/file")?)?
            .mount()
            .id(),
        root_id
    );

    table.enable(docs_id)?;
    table.enable(docs_id)?;
    assert_eq!(
        table
            .resolve(&VirtualPath::parse("/docs/file")?)?
            .mount()
            .id(),
        docs_id
    );
    table.disable(root_id)?;
    assert!(
        matches!(table.resolve(&VirtualPath::root()), Err(MountTableError::Disabled(id)) if id == root_id)
    );
    assert!(
        matches!(table.resolve(&VirtualPath::parse("/other")?), Err(MountTableError::Disabled(id)) if id == root_id)
    );
    assert_eq!(
        table
            .resolve(&VirtualPath::parse("/docs/file")?)?
            .mount()
            .id(),
        docs_id
    );
    table.enable(root_id)?;
    assert_eq!(table.resolve(&VirtualPath::root())?.mount().id(), root_id);
    Ok(())
}

#[test]
fn removal_preserves_nested_mounts_and_releases_only_the_removed_boundary() -> TestResult {
    let mut table = MountTable::new(mount("/", "builtin.local", true)?)?;
    let root_id = table.root().id();
    let parent = mount("/data", "builtin.local", false)?;
    let parent_id = parent.id();
    table.insert(parent, policy)?;
    let child = mount("/data/archive", "acme.s3", true)?;
    let child_id = child.id();
    table.insert(child, policy)?;

    assert_eq!(table.remove(parent_id)?.id(), parent_id);
    assert!(table.get(parent_id).is_none());
    assert_eq!(
        table
            .resolve(&VirtualPath::parse("/data/file")?)?
            .mount()
            .id(),
        root_id
    );
    assert_eq!(
        table
            .resolve(&VirtualPath::parse("/data/archive/file")?)?
            .mount()
            .id(),
        child_id
    );
    assert_eq!(
        table
            .get(child_id)
            .map(|mount| mount.virtual_path().as_str()),
        Some("/data/archive")
    );

    let missing = MountId::new();
    assert!(matches!(table.remove(missing), Err(MountTableError::NotFound(id)) if id == missing));
    assert_eq!(
        table.enable(missing),
        Err(MountTableError::NotFound(missing))
    );
    assert_eq!(
        table.disable(missing),
        Err(MountTableError::NotFound(missing))
    );
    assert_eq!(table.list().len(), 2);
    Ok(())
}

#[test]
fn relative_suffix_can_outlive_the_table() -> TestResult {
    let path = VirtualPath::parse("/docs/report.txt")?;
    let suffix = {
        let mut table = MountTable::new(mount("/", "builtin.local", true)?)?;
        table.insert(mount("/docs", "acme.s3", true)?, policy)?;
        table.resolve(&path)?.relative_path()
    };
    assert_eq!(suffix, "report.txt");
    Ok(())
}

#[test]
fn restoration_preserves_definitions_independently_of_row_order() -> TestResult {
    let records = [
        mount("/", "builtin.local", false)?,
        mount("/docs", "builtin.local", false)?,
        mount("/docs/archive", "acme.s3", true)?,
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let table = MountTable::restore(order.map(|index| records[index].clone()).into(), policy)?;
        assert_eq!(table.list().len(), 3);
        for original in &records {
            let restored = table.get(original.id()).ok_or("mount missing")?;
            assert_eq!(restored.virtual_path(), original.virtual_path());
            assert_eq!(restored.driver_instance(), original.driver_instance());
            assert_eq!(restored.enabled(), original.enabled());
        }
        assert_eq!(
            table
                .resolve(&VirtualPath::parse("/docs/archive/file")?)?
                .mount()
                .id(),
            records[2].id()
        );
        assert!(
            matches!(table.resolve(&VirtualPath::parse("/docs/file")?), Err(MountTableError::Disabled(id)) if id == records[1].id())
        );
    }
    Ok(())
}

#[test]
fn restoration_rejects_missing_root_and_duplicate_definitions() -> TestResult {
    assert!(matches!(
        MountTable::restore(Vec::new(), policy),
        Err(MountTableError::RootRequired)
    ));
    assert!(matches!(
        MountTable::restore(vec![mount("/docs", "builtin.local", true)?], policy),
        Err(MountTableError::RootRequired)
    ));
    assert!(
        matches!(MountTable::restore(vec![mount("/", "builtin.local", true)?, mount("////", "acme.s3", true)?], policy), Err(MountTableError::PathOccupied(path)) if path.is_root())
    );

    let root = mount("/", "builtin.local", true)?;
    let duplicate_id = root.id();
    let duplicate = Mount::builder()
        .id(duplicate_id)
        .virtual_path(VirtualPath::parse("/docs")?)
        .driver_instance(root.driver_instance().clone())
        .enabled(true)
        .build();
    assert!(
        matches!(MountTable::restore(vec![duplicate, root], policy), Err(MountTableError::DuplicateId(id)) if id == duplicate_id)
    );
    assert!(
        matches!(MountTable::restore(vec![mount("/docs/", "builtin.local", true)?, mount("/", "builtin.local", true)?, mount("//docs", "acme.s3", false)?], policy), Err(MountTableError::PathOccupied(path)) if path.as_str() == "/docs")
    );
    Ok(())
}

#[test]
fn restoration_cannot_bypass_driver_policies_in_any_row_order() -> TestResult {
    let records = [
        mount("/", "builtin.local", true)?,
        mount("/cloud", "acme.s3", false)?,
        mount("/cloud/child", "builtin.local", true)?,
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        assert!(matches!(
            MountTable::restore(order.map(|index| records[index].clone()).into(), policy),
            Err(MountTableError::ChildMountsForbidden { driver, path }) if driver.as_str() == "acme.s3" && path.as_str() == "/cloud"
        ));
    }
    assert!(
        matches!(MountTable::restore(vec![mount("/child", "acme.s3", true)?, mount("/", "unavailable.driver", true)?], policy), Err(MountTableError::PolicyUnavailable(key)) if key.as_str() == "unavailable.driver")
    );
    Ok(())
}

#[test]
fn storage_addresses_validate_only_the_suffix_after_namespace_resolution() -> TestResult {
    use ah_vfs::domain::{StorageError, StorageErrorKind, StoragePath, StoragePathError};

    let mut table = MountTable::new(mount("/", "builtin.local", true)?)?;
    table.insert(mount("/cloud:archive", "builtin.local", true)?, policy)?;
    table.insert(mount("/cloud:archive/nested", "acme.s3", true)?, policy)?;
    for (virtual_path, storage_path) in [
        ("/", ""),
        ("/file.txt", "file.txt"),
        ("/cloud:archive", ""),
        ("//cloud:archive///文档/a.txt/", "文档/a.txt"),
        ("/cloud:archive/nested/a.txt", "a.txt"),
    ] {
        let path = VirtualPath::parse(virtual_path)?;
        let resolved = table.resolve(&path)?;
        assert_eq!(resolved.storage_path()?, StoragePath::parse(storage_path)?);
        assert_eq!(resolved.relative_path(), storage_path);
    }
    for virtual_path in [
        "/file:stream",
        "/cloud:archive/file:stream",
        "/cloud:archive-other/file",
    ] {
        let path = VirtualPath::parse(virtual_path)?;
        let resolved = table.resolve(&path)?;
        assert_eq!(
            resolved.storage_path(),
            Err(StoragePathError::InvalidSegment)
        );
        let error = resolved.storage_path().map_err(StorageError::from);
        assert!(matches!(error, Err(error) if error.kind() == StorageErrorKind::InvalidPath));
    }
    // The converted address owns its data independently of both table and request.
    let address = {
        let path = VirtualPath::parse("/cloud:archive/文档/a.txt")?;
        table.resolve(&path)?.storage_path()?
    };
    drop(table);
    assert_eq!(address.as_str(), "文档/a.txt");
    Ok(())
}
