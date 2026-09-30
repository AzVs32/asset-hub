use crate::error::VfsError;
use crate::mount::{Mount, MountId};

/// Persistence boundary for mount definitions.
///
/// A repository stores [`Mount`] values, not bound drivers. Successful
/// changes remain available when another repository instance opens the same
/// storage. Loading a mount does not validate its driver-specific path or make
/// its backend available. Each call observes the current persisted state;
/// another caller or process may change that state between calls. Mount errors
/// are converted into
/// [`VfsError`] with `?` or `.into()`.
#[async_trait::async_trait]
pub trait MountRepository: Send + Sync {
    /// Inserts `mount` without replacing an existing definition.
    ///
    /// Contract:
    /// - Preserve every field of `mount` so a later [`MountRepository::get`]
    ///   returns the same definition unless it has been removed.
    /// - Reject an existing ID with [`crate::mount::MountError::DuplicateId`]
    ///   and an existing virtual path with
    ///   [`crate::mount::MountError::DuplicatePath`]. If both conflict, report
    ///   the ID conflict. This applies across repository instances and processes
    ///   that share the same storage.
    /// - Leave the stored definitions unchanged when insertion fails.
    ///
    /// Storage failures may be wrapped with [`crate::mount::MountError::backend`].
    async fn insert(&self, mount: &Mount) -> Result<(), VfsError>;

    /// Removes the mount with `id`.
    ///
    /// Returns `true` only if a definition was removed. An unknown ID returns
    /// `false` and leaves the repository unchanged. Storage failures may be
    /// wrapped with [`crate::mount::MountError::backend`].
    async fn remove(&self, id: MountId) -> Result<bool, VfsError>;

    /// Loads the mount with `id`.
    ///
    /// Returns `None` when the ID is absent. A stored value that cannot be
    /// represented as a [`Mount`] must return
    /// [`crate::mount::MountError::InvalidStoredMount`]; storage failures may
    /// be wrapped with [`crate::mount::MountError::backend`].
    async fn get(&self, id: MountId) -> Result<Option<Mount>, VfsError>;

    /// Loads all mounts in ascending virtual-path order.
    ///
    /// Return each stored definition exactly once. A stored value that cannot
    /// be represented as a [`Mount`] must return
    /// [`crate::mount::MountError::InvalidStoredMount`]; storage failures may
    /// be wrapped with [`crate::mount::MountError::backend`].
    async fn list(&self) -> Result<Vec<Mount>, VfsError>;
}
