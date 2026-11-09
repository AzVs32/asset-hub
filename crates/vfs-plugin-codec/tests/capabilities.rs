use ah_vfs::application::driver_error::LifecycleFailure;
use ah_vfs_storage::{
    AttributeSupport, Capabilities, DirectorySupport, ReadSupport, RenameSupport, WriteModes,
    WriteSupport,
};
use std::error::Error;
use vfs_plugin_codec::{capabilities, decode_capabilities};
use vfs_plugin_protocol::Capability;

#[test]
fn contradictory_wire_capabilities_fail_before_constructing_a_declaration()
-> Result<(), Box<dyn Error>> {
    for wire in [
        vec![Capability::AtomicPublish as i32],
        vec![Capability::EmptyDirs as i32],
        vec![Capability::CreateDir as i32],
        vec![Capability::PermissionsWrite as i32],
        vec![Capability::Prefix as i32, Capability::Hierarchical as i32],
    ] {
        let error = decode_capabilities(&wire)
            .err()
            .ok_or("accepted invalid declaration")?;
        assert_eq!(
            error
                .lifecycle_error()
                .ok_or("invalid declaration")?
                .failure(),
            LifecycleFailure::InvalidDeclaration
        );
    }
    Ok(())
}

#[test]
fn valid_capabilities_round_trip_and_unknown_optional_values_do_not_enable_features()
-> Result<(), Box<dyn Error>> {
    for directories in [
        DirectorySupport::hierarchical(true, true),
        DirectorySupport::prefix(true, true),
    ] {
        let declared = Capabilities::default()
            .with_stat(true)
            .with_list(true)
            .with_read_link(true)
            .with_read(ReadSupport::Range)
            .with_write(WriteSupport::staged(WriteModes::Both, true))
            .with_directories(directories)
            .with_remove(true)
            .with_rename(RenameSupport::Both)
            .with_permissions(AttributeSupport::ReadWrite)
            .with_xattrs(AttributeSupport::Read);
        let mut wire = capabilities(declared);
        wire.push(i32::MAX);
        assert_eq!(decode_capabilities(&wire)?, declared);
    }
    assert_eq!(decode_capabilities(&[i32::MAX])?, Capabilities::default());
    Ok(())
}
