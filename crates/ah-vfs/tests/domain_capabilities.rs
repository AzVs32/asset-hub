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
fn construction_errors_retain_their_validation_cause() -> TestResult {
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
        assert_eq!(error.kind(), StorageErrorKind::ProtocolViolation);
        assert_eq!(
            error
                .source()
                .and_then(|e| e.downcast_ref::<StorageCapabilitiesError>()),
            Some(&expected)
        );
    }
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
    assert_eq!(reduced.dir_model(), DirModel::None);
    assert_eq!(reduced.xattrs(), AttributeSupport::None);
    assert_eq!(original, full()?);
    Ok(())
}
