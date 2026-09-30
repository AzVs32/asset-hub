/// Public entry point for managing mounts.
///
/// This is an API placeholder. Method arguments, return types, and shared
/// state will be defined when mount behavior is designed.
#[derive(Debug)]
pub struct MountService {
    _private: (),
}

impl MountService {
    /// Mounts a backend in the virtual namespace.
    pub async fn mount(&self) {
        todo!("MountService::mount")
    }

    /// Removes a mount from the virtual namespace.
    pub async fn unmount(&self) {
        todo!("MountService::unmount")
    }

    /// Lists configured mounts.
    pub async fn list_mounts(&self) {
        todo!("MountService::list_mounts")
    }

    /// Returns information about one mount.
    pub async fn mount_info(&self) {
        todo!("MountService::mount_info")
    }

    /// Resolves a virtual path to its covering mount.
    pub async fn resolve_mount(&self) {
        todo!("MountService::resolve_mount")
    }
}
