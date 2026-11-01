use super::{DriverInstanceRef, MountId, VirtualPath};
use bon::Builder;
use getset::{CopyGetters, Getters, Setters};

/// A mount definition linking a virtual location to a host-managed instance.
///
/// Disabling a mount preserves its definition. Routing and mount-conflict checks
/// belong to the filesystem that manages the collection of mounts.
#[derive(Clone, Debug, Builder, Getters, CopyGetters, Setters)]
pub struct Mount {
    #[getset(get_copy = "pub")]
    id: MountId,
    #[getset(get = "pub")]
    virtual_path: VirtualPath,
    #[getset(get = "pub")]
    driver_instance: DriverInstanceRef,
    #[getset(get_copy = "pub", set = "pub")]
    enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::super::{DriverKey, InstanceId};
    use super::*;

    #[test]
    fn disabling_preserves_mount_identity_location_and_instance()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = VirtualPath::parse("/documents")?;
        let id = MountId::new();
        let instance =
            DriverInstanceRef::new(DriverKey::parse("builtin.local")?, InstanceId::parse("42")?);
        let mut mount = Mount::builder()
            .id(id)
            .virtual_path(path.clone())
            .driver_instance(instance.clone())
            .enabled(true)
            .build();

        mount.set_enabled(false);

        assert!(!mount.enabled());
        assert_eq!(mount.id(), id);
        assert_eq!(mount.virtual_path(), &path);
        assert_eq!(mount.driver_instance(), &instance);

        mount.set_enabled(true);
        assert!(mount.enabled());
        assert_eq!(mount.driver_instance(), &instance);
        Ok(())
    }
}
