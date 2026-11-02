use ah_vfs::domain::{
    AttributeSupport, DirModel, FileReadSupport, FileWriteSupport, RenameSupport,
    StorageCapabilities, StorageCapabilitiesError, StorageErrorKind,
};
use std::error::Error;
use vfs_plugin_codec::{capabilities, decode_capabilities};
use vfs_plugin_protocol::Capability;

#[test]
fn contradictory_wire_capabilities_preserve_the_domain_validation_cause()
-> Result<(), Box<dyn Error>> {
    for (wire, expected) in [
        (
            vec![Capability::AtomicPublish as i32],
            StorageCapabilitiesError::AtomicPublishWithoutWrite,
        ),
        (
            vec![Capability::EmptyDirs as i32],
            StorageCapabilitiesError::EmptyDirsWithoutDirModel,
        ),
        (
            vec![Capability::CreateDir as i32],
            StorageCapabilitiesError::CreateDirWithoutDirModel,
        ),
    ] {
        let error = decode_capabilities(&wire)
            .err()
            .ok_or("accepted invalid wire capabilities")?;
        let storage = error.storage_error().ok_or("missing storage error")?;
        assert_eq!(storage.kind(), StorageErrorKind::InvalidData);
        assert_eq!(
            storage
                .source()
                .and_then(|e| e.downcast_ref::<StorageCapabilitiesError>()),
            Some(&expected)
        );
    }
    Ok(())
}

#[test]
fn valid_capabilities_round_trip_and_unknown_optional_values_do_not_enable_features()
-> Result<(), Box<dyn Error>> {
    for dir_model in [DirModel::Hierarchical, DirModel::Prefix] {
        let declared = StorageCapabilities {
            stat_entry: true,
            list_entries: true,
            read_file: FileReadSupport::Range,
            write_file: FileWriteSupport {
                create_new: true,
                replace: true,
                atomic_publish: true,
            },
            dir_model,
            persistent_empty_dirs: true,
            create_dir: true,
            remove_entry: true,
            rename: RenameSupport {
                no_replace: true,
                replace: true,
            },
            permissions: AttributeSupport::ReadWrite,
            xattrs: AttributeSupport::Read,
        };
        let mut wire = capabilities(declared);
        wire.push(i32::MAX);
        assert_eq!(decode_capabilities(&wire)?, declared);
    }
    assert_eq!(
        decode_capabilities(&[i32::MAX])?,
        StorageCapabilities::default()
    );
    Ok(())
}
