/// Public entry point for virtual filesystem operations.
///
/// This is an API placeholder. Method arguments, return types, and shared
/// state will be defined when VFS behavior is designed.
#[derive(Debug)]
pub struct VfsService {
    _private: (),
}

impl VfsService {
    /// Lists the children of a virtual directory.
    pub fn list(&self) {
        todo!("VfsService::list")
    }

    /// Reads a virtual file.
    pub fn read(&self) {
        todo!("VfsService::read")
    }

    /// Writes a virtual file.
    pub fn write(&self) {
        todo!("VfsService::write")
    }

    /// Creates a virtual directory.
    pub fn mkdir(&self) {
        todo!("VfsService::mkdir")
    }

    /// Removes a virtual entry.
    pub fn remove(&self) {
        todo!("VfsService::remove")
    }

    /// Renames a virtual entry.
    pub fn rename(&self) {
        todo!("VfsService::rename")
    }
}
