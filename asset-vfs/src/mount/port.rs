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
    /// - IDs are unique. Only enabled mounts must have unique virtual paths;
    ///   any number of disabled definitions can share a path.
    /// - Report DuplicateId before DuplicatePath when both conflict, including
    ///   across repository instances and processes sharing the storage.
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

    /// Loads all mounts ordered by virtual path, enabled first, then ID.
    ///
    /// Return each stored definition exactly once. A stored value that cannot
    /// be represented as a [`Mount`] must return
    /// [`crate::mount::MountError::InvalidStoredMount`]; storage failures may
    /// be wrapped with [`crate::mount::MountError::backend`].
    async fn list(&self) -> Result<Vec<Mount>, VfsError>;

    /// Starts a serialized write transaction.
    ///
    /// Reads and writes inside it observe one consistent state. Other writers
    /// must wait until commit or rollback, including writers using another
    /// repository instance. Dropping the transaction rolls back all changes.
    /// The service applies business rules inside this boundary; the repository
    /// enforces storage constraints without interpreting driver declarations.
    async fn begin(&self) -> Result<Box<dyn MountTransaction>, VfsError>;
}

/// Storage operations in a transaction owned by a mount use case.
#[async_trait::async_trait]
pub trait MountTransaction: Send {
    async fn list(&mut self) -> Result<Vec<Mount>, VfsError>;
    /// Inserts with the same constraints as MountRepository::insert.
    async fn insert(&mut self, mount: &Mount) -> Result<(), VfsError>;
    /// Replaces a definition by ID; returns false if absent. Enforces enabled
    /// path uniqueness and preserves storage on failure.
    async fn update(&mut self, mount: &Mount) -> Result<bool, VfsError>;
    async fn commit(self: Box<Self>) -> Result<(), VfsError>;
}
