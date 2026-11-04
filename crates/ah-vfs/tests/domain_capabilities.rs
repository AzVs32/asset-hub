use ah_vfs::domain::storage::{
    AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
    StorageCapabilities, StorageCapabilitiesBuilder, StorageCapabilitiesError, StorageError,
    StorageErrorKind,
};
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;
fn full() -> Result<StorageCapabilities, StorageCapabilitiesError> {
    StorageCapabilities::builder()
        .stat_entry(true)
        .list_entries(true)
        .read_file(FileReadSupport::Range)
        .write_file(FileWriteSupport {
            create_new: true,
            replace: true,
            atomic_publish: true,
        })
        .dir_model(DirModel::Hierarchical)
        .persistent_empty_dirs(true)
        .create_dir(true)
        .remove_entry(true)
        .rename(RenameSupport {
            no_replace: true,
            replace: true,
        })
        .permissions(AttributeSupport::ReadWrite)
        .xattrs(AttributeSupport::ReadWrite)
        .build()
}

type Toggle = fn(StorageCapabilitiesBuilder, bool) -> StorageCapabilitiesBuilder;
const FLAGS: [(&str, Toggle); 10] = [
    ("stat", StorageCapabilitiesBuilder::stat_entry),
    ("list", StorageCapabilitiesBuilder::list_entries),
    ("write new", |c, v| {
        c.write_file(FileWriteSupport {
            create_new: v,
            replace: true,
            atomic_publish: true,
        })
    }),
    ("write replace", |c, v| {
        c.write_file(FileWriteSupport {
            create_new: true,
            replace: v,
            atomic_publish: true,
        })
    }),
    ("atomic publish", |c, v| {
        c.write_file(FileWriteSupport {
            create_new: true,
            replace: true,
            atomic_publish: v,
        })
    }),
    (
        "empty dirs",
        StorageCapabilitiesBuilder::persistent_empty_dirs,
    ),
    ("create dir", StorageCapabilitiesBuilder::create_dir),
    ("remove", StorageCapabilitiesBuilder::remove_entry),
    ("rename new", |c, v| {
        c.rename(RenameSupport {
            no_replace: v,
            replace: true,
        })
    }),
    ("rename replace", |c, v| {
        c.rename(RenameSupport {
            no_replace: true,
            replace: v,
        })
    }),
];

#[test]
fn defaults_disable_every_optional_capability() -> TestResult {
    let caps = StorageCapabilities::default();
    assert_eq!(StorageCapabilities::builder().build()?, caps);
    assert_eq!(StorageCapabilitiesBuilder::default().build()?, caps);
    assert_eq!(caps.to_builder().build()?, caps);
    assert!(!caps.stat_entry());
    assert!(!caps.list_entries());
    assert_eq!(caps.read_file(), FileReadSupport::None);
    assert_eq!(caps.write_file(), FileWriteSupport::default());
    assert_eq!(caps.dir_model(), DirModel::None);
    assert!(!caps.persistent_empty_dirs());
    assert!(!caps.create_dir());
    assert!(!caps.remove_entry());
    assert_eq!(caps.rename(), RenameSupport::default());
    assert_eq!(caps.permissions(), AttributeSupport::None);
    assert_eq!(caps.xattrs(), AttributeSupport::None);
    Ok(())
}

#[test]
fn every_write_mode_combination_is_checked_during_construction() {
    for create_new in [false, true] {
        for replace in [false, true] {
            for atomic_publish in [false, true] {
                let write = FileWriteSupport {
                    create_new,
                    replace,
                    atomic_publish,
                };
                let expected = if atomic_publish && !create_new && !replace {
                    Err(StorageCapabilitiesError::AtomicPublishWithoutWrite)
                } else {
                    Ok(())
                };
                assert_eq!(write.validate(), expected);
                let result = StorageCapabilities::builder().write_file(write).build();
                assert_eq!(result.map(|_| ()), expected);
            }
        }
    }
}

#[test]
fn directory_guarantees_require_a_model_but_operations_remain_independent() -> TestResult {
    for dir_model in [DirModel::None, DirModel::Hierarchical, DirModel::Prefix] {
        for persistent_empty_dirs in [false, true] {
            for create_dir in [false, true] {
                let result = StorageCapabilities::builder()
                    .dir_model(dir_model)
                    .persistent_empty_dirs(persistent_empty_dirs)
                    .create_dir(create_dir)
                    .build();
                let expected = match (dir_model, persistent_empty_dirs, create_dir) {
                    (DirModel::None, true, _) => {
                        Err(StorageCapabilitiesError::EmptyDirsWithoutDirModel)
                    }
                    (DirModel::None, false, true) => {
                        Err(StorageCapabilitiesError::CreateDirWithoutDirModel)
                    }
                    _ => Ok(()),
                };
                assert_eq!(result.map(|_| ()), expected);
            }
        }
    }
    let flat = StorageCapabilities::builder()
        .list_entries(true)
        .read_file(FileReadSupport::Sequential)
        .remove_entry(true)
        .build()?;
    assert!(!flat.stat_entry());
    Ok(())
}

#[test]
fn each_boolean_guarantee_is_checked_in_both_coverage_directions() -> TestResult {
    for (name, toggle) in FLAGS {
        for declared in [false, true] {
            for actual in [false, true] {
                let upper = toggle(full()?.to_builder(), declared).build()?;
                let opened = toggle(full()?.to_builder(), actual).build()?;
                assert_eq!(upper.supports(opened), declared || !actual, "{name}");
            }
        }
    }
    Ok(())
}

#[test]
fn read_and_attribute_levels_cover_exactly_their_lower_levels() -> TestResult {
    let reads = [
        FileReadSupport::None,
        FileReadSupport::Sequential,
        FileReadSupport::Range,
    ];
    let attrs = [
        AttributeSupport::None,
        AttributeSupport::Read,
        AttributeSupport::ReadWrite,
    ];
    for declared in 0..3 {
        assert_eq!(attrs[declared].can_read(), declared >= 1);
        assert_eq!(attrs[declared].can_write(), declared == 2);
        for actual in 0..3 {
            let upper = StorageCapabilities::builder()
                .read_file(reads[declared])
                .build()?;
            let opened = StorageCapabilities::builder()
                .read_file(reads[actual])
                .build()?;
            assert_eq!(upper.supports(opened), actual <= declared);
            for attribute in [
                StorageCapabilitiesBuilder::permissions,
                StorageCapabilitiesBuilder::xattrs,
            ] {
                let upper = attribute(StorageCapabilities::builder(), attrs[declared]).build()?;
                let opened = attribute(StorageCapabilities::builder(), attrs[actual]).build()?;
                assert_eq!(upper.supports(opened), actual <= declared);
            }
        }
    }
    Ok(())
}

#[test]
fn directory_models_are_not_interchangeable() -> TestResult {
    for declared in [DirModel::None, DirModel::Hierarchical, DirModel::Prefix] {
        for actual in [DirModel::None, DirModel::Hierarchical, DirModel::Prefix] {
            let upper = StorageCapabilities::builder().dir_model(declared).build()?;
            let opened = StorageCapabilities::builder().dir_model(actual).build()?;
            assert_eq!(
                upper.supports(opened),
                actual == DirModel::None || actual == declared
            );
        }
    }
    Ok(())
}

#[test]
fn construction_errors_are_distinct_from_insufficient_coverage() -> TestResult {
    for (builder, expected) in [
        (
            StorageCapabilities::builder().write_file(FileWriteSupport {
                atomic_publish: true,
                ..Default::default()
            }),
            StorageCapabilitiesError::AtomicPublishWithoutWrite,
        ),
        (
            StorageCapabilities::builder().persistent_empty_dirs(true),
            StorageCapabilitiesError::EmptyDirsWithoutDirModel,
        ),
        (
            StorageCapabilities::builder().create_dir(true),
            StorageCapabilitiesError::CreateDirWithoutDirModel,
        ),
    ] {
        assert_eq!(builder.build(), Err(expected));
        let error = StorageError::from(expected);
        assert_eq!(error.kind(), StorageErrorKind::InvalidData);
        assert_eq!(
            error
                .source()
                .and_then(|e| e.downcast_ref::<StorageCapabilitiesError>()),
            Some(&expected)
        );
    }
    assert!(!StorageCapabilities::default().supports(full()?));
    assert!(full()?.supports(StorageCapabilities::default()));
    Ok(())
}

#[test]
fn rebuilding_revalidates_and_getters_cannot_mutate_the_original() -> TestResult {
    let original = full()?;
    assert_eq!(
        original.to_builder().dir_model(DirModel::None).build(),
        Err(StorageCapabilitiesError::EmptyDirsWithoutDirModel)
    );
    let mut write = original.write_file();
    write.create_new = false;
    write.replace = false;
    assert_eq!(
        original.to_builder().write_file(write).build(),
        Err(StorageCapabilitiesError::AtomicPublishWithoutWrite)
    );
    assert_eq!(original, full()?);
    let reduced = original
        .to_builder()
        .persistent_empty_dirs(false)
        .create_dir(false)
        .dir_model(DirModel::None)
        .xattrs(AttributeSupport::None)
        .build()?;
    assert!(original.supports(reduced));
    assert!(!reduced.supports(original));
    Ok(())
}

#[test]
fn coverage_is_a_partial_order_for_valid_combined_declarations() -> TestResult {
    let mut samples = vec![StorageCapabilities::default(), full()?];
    for (_, toggle) in FLAGS {
        samples.push(toggle(full()?.to_builder(), false).build()?);
    }
    for dir_model in [DirModel::None, DirModel::Hierarchical, DirModel::Prefix] {
        for read_file in [
            FileReadSupport::None,
            FileReadSupport::Sequential,
            FileReadSupport::Range,
        ] {
            for attribute in [
                AttributeSupport::None,
                AttributeSupport::Read,
                AttributeSupport::ReadWrite,
            ] {
                samples.push(
                    StorageCapabilities::builder()
                        .dir_model(dir_model)
                        .read_file(read_file)
                        .permissions(attribute)
                        .xattrs(attribute)
                        .build()?,
                );
            }
        }
    }
    for &a in &samples {
        assert!(a.supports(a));
        for &b in &samples {
            if !a.supports(b) {
                continue;
            }
            if b.supports(a) {
                assert_eq!(a, b);
            }
            for &c in &samples {
                if b.supports(c) {
                    assert!(a.supports(c), "{a:?} >= {b:?} >= {c:?}");
                }
            }
        }
    }
    Ok(())
}
