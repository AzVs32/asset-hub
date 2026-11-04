//! Transport-independent storage capabilities and semantic guarantees.

use getset::CopyGetters;
use thiserror::Error;

/// Prefix dirs do not imply persistent empty dirs. Adapters must document marker
/// creation and file/prefix collisions instead of silently pretending POSIX dirs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DirModel {
    /// No child-directory semantics. Root listing is independently advertised.
    #[default]
    None,
    Hierarchical,
    Prefix,
}
/// Range includes arbitrary offset/length reads. Neither variant promises snapshots
/// across separate streams or immunity to external writes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileReadSupport {
    #[default]
    None,
    Sequential,
    Range,
}
/// All supported writes stage until commit. Atomic publication is independent of
/// power-loss durability. Unsupported modes must fail before creating staging data.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FileWriteSupport {
    pub create_new: bool,
    pub replace: bool,
    pub atomic_publish: bool,
}

impl FileWriteSupport {
    /// Reject a publication guarantee without any supported write mode.
    pub fn validate(self) -> Result<(), StorageCapabilitiesError> {
        if self.atomic_publish && !self.create_new && !self.replace {
            return Err(StorageCapabilitiesError::AtomicPublishWithoutWrite);
        }
        Ok(())
    }
}
/// Both variants require atomic rename within the supported scope, never copy/delete.
/// Cross-device requests may fail even when the instance advertises support.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenameSupport {
    pub no_replace: bool,
    pub replace: bool,
}
/// Typed attribute support, separate from caller authorization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AttributeSupport {
    #[default]
    None,
    Read,
    ReadWrite,
}
impl AttributeSupport {
    pub fn can_read(self) -> bool {
        self != Self::None
    }
    pub fn can_write(self) -> bool {
        self == Self::ReadWrite
    }
}
/// A driver-wide declaration of supported operations and successful-operation
/// guarantees. All instances share it; authorization, instance and path limitations
/// are reported by operations, not by instance-specific capability declarations.
/// Unsupported operations fail before side effects; guarantees cannot be weakened.
/// Unknown optional capabilities default to unsupported.
///
/// Every value is semantically valid. Use [`Self::builder`] to assemble a declaration;
/// [`StorageCapabilitiesBuilder::build`] rejects contradictions before returning it.
/// All fields are private and getters return copies. A declaration is not a live
/// handle, a backend probe or proof that an adapter implements its promises.
/// New optional capabilities default to unsupported.
///
/// External adapters use the builder instead of exhaustive struct literals:
///
/// ```
/// use ah_vfs::domain::storage::{FileReadSupport, StorageCapabilities};
/// let capabilities = StorageCapabilities::builder()
///     .stat_entry(true)
///     .read_file(FileReadSupport::Range)
///     .build()?;
/// assert!(capabilities.stat_entry());
/// # Ok::<(), ah_vfs::domain::storage::StorageCapabilitiesError>(())
/// ```
///
/// Fields cannot be changed after construction:
///
/// ```compile_fail
/// use ah_vfs::domain::storage::StorageCapabilities;
/// let mut capabilities = StorageCapabilities::default();
/// capabilities.create_dir = true;
/// ```
///
/// Struct literals cannot bypass construction checks:
///
/// ```compile_fail
/// use ah_vfs::domain::storage::StorageCapabilities;
/// let capabilities = StorageCapabilities { stat_entry: true, ..Default::default() };
/// ```
///
/// A build result cannot be used as a capability value without handling failure:
///
/// ```compile_fail
/// use ah_vfs::domain::storage::StorageCapabilities;
/// let capabilities = StorageCapabilities::builder().create_dir(true).build();
/// capabilities.stat_entry();
/// ```
/// Independent operations do not imply each other: listing need not imply stat,
/// and persistent directories need not imply permission to create directories.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, CopyGetters)]
#[getset(get_copy = "pub")]
#[non_exhaustive]
pub struct StorageCapabilities {
    stat_entry: bool,
    list_entries: bool,
    read_file: FileReadSupport,
    write_file: FileWriteSupport,
    dir_model: DirModel,
    persistent_empty_dirs: bool,
    create_dir: bool,
    remove_entry: bool,
    rename: RenameSupport,
    permissions: AttributeSupport,
    xattrs: AttributeSupport,
}

/// Contradictory capability declarations, independent of any plugin transport.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum StorageCapabilitiesError {
    #[error("atomic publication requires at least one file write mode")]
    AtomicPublishWithoutWrite,
    #[error("persistent empty directories require a directory model")]
    EmptyDirsWithoutDirModel,
    #[error("directory creation requires a directory model")]
    CreateDirWithoutDirModel,
}

impl StorageCapabilities {
    /// Start a declaration with every optional capability unsupported.
    pub fn builder() -> StorageCapabilitiesBuilder {
        StorageCapabilitiesBuilder::default()
    }

    /// Copy this declaration into an editable builder. Rebuilding rechecks all rules.
    pub fn to_builder(self) -> StorageCapabilitiesBuilder {
        StorageCapabilitiesBuilder {
            stat_entry: self.stat_entry,
            list_entries: self.list_entries,
            read_file: self.read_file,
            write_file: self.write_file,
            dir_model: self.dir_model,
            persistent_empty_dirs: self.persistent_empty_dirs,
            create_dir: self.create_dir,
            remove_entry: self.remove_entry,
            rename: self.rename,
            permissions: self.permissions,
            xattrs: self.xattrs,
        }
    }
}

/// An editable declaration that cannot be used as a storage capability value.
///
/// Every field defaults to unsupported. Setters may be repeated, including after
/// [`StorageCapabilities::to_builder`]; [`Self::build`] validates the complete
/// declaration without normalizing or weakening requested guarantees.
#[derive(Clone, Copy, Debug, Default)]
#[must_use = "finish capability construction with build() and handle its result"]
pub struct StorageCapabilitiesBuilder {
    stat_entry: bool,
    list_entries: bool,
    read_file: FileReadSupport,
    write_file: FileWriteSupport,
    dir_model: DirModel,
    persistent_empty_dirs: bool,
    create_dir: bool,
    remove_entry: bool,
    rename: RenameSupport,
    permissions: AttributeSupport,
    xattrs: AttributeSupport,
}

impl StorageCapabilitiesBuilder {
    /// Set the `stat_entry` declaration.
    pub fn stat_entry(mut self, value: bool) -> Self {
        self.stat_entry = value;
        self
    }

    /// Set the `list_entries` declaration.
    pub fn list_entries(mut self, value: bool) -> Self {
        self.list_entries = value;
        self
    }

    /// Set the `read_file` declaration.
    pub fn read_file(mut self, value: FileReadSupport) -> Self {
        self.read_file = value;
        self
    }

    /// Set the `write_file` declaration.
    pub fn write_file(mut self, value: FileWriteSupport) -> Self {
        self.write_file = value;
        self
    }

    /// Set the `dir_model` declaration.
    pub fn dir_model(mut self, value: DirModel) -> Self {
        self.dir_model = value;
        self
    }

    /// Set the `persistent_empty_dirs` declaration.
    pub fn persistent_empty_dirs(mut self, value: bool) -> Self {
        self.persistent_empty_dirs = value;
        self
    }

    /// Set the `create_dir` declaration.
    pub fn create_dir(mut self, value: bool) -> Self {
        self.create_dir = value;
        self
    }

    /// Set the `remove_entry` declaration.
    pub fn remove_entry(mut self, value: bool) -> Self {
        self.remove_entry = value;
        self
    }

    /// Set the `rename` declaration.
    pub fn rename(mut self, value: RenameSupport) -> Self {
        self.rename = value;
        self
    }

    /// Set the `permissions` declaration.
    pub fn permissions(mut self, value: AttributeSupport) -> Self {
        self.permissions = value;
        self
    }

    /// Set the `xattrs` declaration.
    pub fn xattrs(mut self, value: AttributeSupport) -> Self {
        self.xattrs = value;
        self
    }

    /// Validate the entire declaration and return an immutable capability value.
    ///
    /// # Errors
    ///
    /// Reject atomic publication without a write mode, or empty-directory and
    /// directory-creation guarantees without a directory model. No backend is probed.
    pub fn build(self) -> Result<StorageCapabilities, StorageCapabilitiesError> {
        self.write_file.validate()?;
        if self.dir_model == DirModel::None {
            if self.persistent_empty_dirs {
                return Err(StorageCapabilitiesError::EmptyDirsWithoutDirModel);
            }
            if self.create_dir {
                return Err(StorageCapabilitiesError::CreateDirWithoutDirModel);
            }
        }
        Ok(StorageCapabilities {
            stat_entry: self.stat_entry,
            list_entries: self.list_entries,
            read_file: self.read_file,
            write_file: self.write_file,
            dir_model: self.dir_model,
            persistent_empty_dirs: self.persistent_empty_dirs,
            create_dir: self.create_dir,
            remove_entry: self.remove_entry,
            rename: self.rename,
            permissions: self.permissions,
            xattrs: self.xattrs,
        })
    }
}
