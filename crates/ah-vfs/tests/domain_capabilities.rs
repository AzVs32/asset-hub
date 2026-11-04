use ah_vfs::domain::storage::{
    AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
    StorageCapabilities, StorageCapabilitiesError, StorageError, StorageErrorKind,
};
use std::error::Error;

fn full() -> StorageCapabilities {
    StorageCapabilities {
        stat_entry: true,
        list_entries: true,
        read_file: FileReadSupport::Range,
        write_file: FileWriteSupport {
            create_new: true,
            replace: true,
            atomic_publish: true,
        },
        dir_model: DirModel::Hierarchical,
        persistent_empty_dirs: true,
        create_dir: true,
        remove_entry: true,
        rename: RenameSupport {
            no_replace: true,
            replace: true,
        },
        permissions: AttributeSupport::ReadWrite,
        xattrs: AttributeSupport::ReadWrite,
    }
}

type Toggle = fn(&mut StorageCapabilities, bool);
const FLAGS: [(&str, Toggle); 10] = [
    ("stat", |c, v| c.stat_entry = v),
    ("list", |c, v| c.list_entries = v),
    ("write new", |c, v| c.write_file.create_new = v),
    ("write replace", |c, v| c.write_file.replace = v),
    ("atomic publish", |c, v| c.write_file.atomic_publish = v),
    ("empty dirs", |c, v| c.persistent_empty_dirs = v),
    ("create dir", |c, v| c.create_dir = v),
    ("remove", |c, v| c.remove_entry = v),
    ("rename new", |c, v| c.rename.no_replace = v),
    ("rename replace", |c, v| c.rename.replace = v),
];

#[test]
fn every_write_mode_combination_has_an_explicit_validity_rule() {
    for create_new in [false, true] {
        for replace in [false, true] {
            for atomic_publish in [false, true] {
                let write = FileWriteSupport {
                    create_new,
                    replace,
                    atomic_publish,
                };
                let caps = StorageCapabilities {
                    write_file: write,
                    ..Default::default()
                };
                let expected = if atomic_publish && !create_new && !replace {
                    Err(StorageCapabilitiesError::AtomicPublishWithoutWrite)
                } else {
                    Ok(())
                };
                assert_eq!(write.validate(), expected);
                assert_eq!(caps.validate(), expected);
            }
        }
    }
}

#[test]
fn directory_guarantees_require_a_model_but_operations_remain_independent() {
    for dir_model in [DirModel::None, DirModel::Hierarchical, DirModel::Prefix] {
        for persistent_empty_dirs in [false, true] {
            for create_dir in [false, true] {
                let caps = StorageCapabilities {
                    dir_model,
                    persistent_empty_dirs,
                    create_dir,
                    ..Default::default()
                };
                let expected = match (dir_model, persistent_empty_dirs, create_dir) {
                    (DirModel::None, true, _) => {
                        Err(StorageCapabilitiesError::EmptyDirsWithoutDirModel)
                    }
                    (DirModel::None, false, true) => {
                        Err(StorageCapabilitiesError::CreateDirWithoutDirModel)
                    }
                    _ => Ok(()),
                };
                assert_eq!(caps.validate(), expected, "{caps:?}");
            }
        }
    }

    // Root enumeration and independent read/remove operations need no child dirs.
    let flat = StorageCapabilities {
        list_entries: true,
        read_file: FileReadSupport::Sequential,
        remove_entry: true,
        ..Default::default()
    };
    assert_eq!(flat.validate(), Ok(()));
    assert!(!flat.stat_entry);
}

#[test]
fn each_boolean_guarantee_is_checked_in_both_coverage_directions() {
    for (name, toggle) in FLAGS {
        for declared in [false, true] {
            for actual in [false, true] {
                let mut upper = full();
                let mut opened = full();
                toggle(&mut upper, declared);
                toggle(&mut opened, actual);
                assert_eq!(upper.validate(), Ok(()));
                assert_eq!(opened.validate(), Ok(()));
                assert_eq!(upper.supports(opened), declared || !actual, "{name}");
            }
        }
    }
}

#[test]
fn read_and_attribute_levels_cover_exactly_their_lower_levels() {
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
            let upper = StorageCapabilities {
                read_file: reads[declared],
                ..Default::default()
            };
            let opened = StorageCapabilities {
                read_file: reads[actual],
                ..Default::default()
            };
            assert_eq!(upper.supports(opened), actual <= declared);
            for attribute in [
                (|c: &mut StorageCapabilities, v| c.permissions = v)
                    as fn(&mut StorageCapabilities, AttributeSupport),
                (|c: &mut StorageCapabilities, v| c.xattrs = v)
                    as fn(&mut StorageCapabilities, AttributeSupport),
            ] {
                let mut upper = StorageCapabilities::default();
                let mut opened = StorageCapabilities::default();
                attribute(&mut upper, attrs[declared]);
                attribute(&mut opened, attrs[actual]);
                assert_eq!(upper.supports(opened), actual <= declared);
            }
        }
    }
}

#[test]
fn directory_models_are_not_interchangeable() {
    for declared in [DirModel::None, DirModel::Hierarchical, DirModel::Prefix] {
        for actual in [DirModel::None, DirModel::Hierarchical, DirModel::Prefix] {
            let upper = StorageCapabilities {
                dir_model: declared,
                ..Default::default()
            };
            let opened = StorageCapabilities {
                dir_model: actual,
                ..Default::default()
            };
            assert_eq!(
                upper.supports(opened),
                actual == DirModel::None || actual == declared
            );
        }
    }
}

#[test]
fn invalid_declarations_are_rejected_even_when_both_sides_agree() {
    let invalid = [
        (
            StorageCapabilities {
                write_file: FileWriteSupport {
                    atomic_publish: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            StorageCapabilitiesError::AtomicPublishWithoutWrite,
        ),
        (
            StorageCapabilities {
                persistent_empty_dirs: true,
                ..Default::default()
            },
            StorageCapabilitiesError::EmptyDirsWithoutDirModel,
        ),
        (
            StorageCapabilities {
                create_dir: true,
                ..Default::default()
            },
            StorageCapabilitiesError::CreateDirWithoutDirModel,
        ),
    ];
    for (caps, expected) in invalid {
        assert_eq!(caps.validate(), Err(expected));
        assert!(!caps.supports(caps));
        assert!(!full().supports(caps));
        assert!(!caps.supports(StorageCapabilities::default()));
        let error = StorageError::from(expected);
        assert_eq!(error.kind(), StorageErrorKind::InvalidData);
        assert_eq!(
            error
                .source()
                .and_then(|e| e.downcast_ref::<StorageCapabilitiesError>()),
            Some(&expected)
        );
    }
}

#[test]
fn coverage_is_a_partial_order_for_valid_combined_declarations() {
    let mut samples = vec![StorageCapabilities::default(), full()];
    for (_, toggle) in FLAGS {
        let mut caps = full();
        toggle(&mut caps, false);
        samples.push(caps);
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
                samples.push(StorageCapabilities {
                    dir_model,
                    read_file,
                    permissions: attribute,
                    xattrs: attribute,
                    ..Default::default()
                });
            }
        }
    }
    for &a in &samples {
        assert_eq!(a.validate(), Ok(()));
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
}
