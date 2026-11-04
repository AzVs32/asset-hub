//! Transport-independent storage capabilities and semantic guarantees.

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
/// Driver declarations are implementation upper bounds. Storage reports effective
/// instance/platform support, not authorization or success for every path/device.
/// Unsupported operations fail before side effects; guarantees cannot be weakened.
/// Unknown optional capabilities default to unsupported.
///
/// This is a declaration, not a validated handle: public fields allow adapters to
/// assemble or decode it without a transport-specific builder. Call [`Self::validate`]
/// at every acceptance boundary, before initialization or storage I/O. Validation
/// rejects contradictions; it never infers capabilities or silently repairs data.
/// Independent operations do not imply each other: listing need not imply stat,
/// and persistent directories need not imply permission to create directories.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StorageCapabilities {
    pub stat_entry: bool,
    pub list_entries: bool,
    pub read_file: FileReadSupport,
    pub write_file: FileWriteSupport,
    pub dir_model: DirModel,
    pub persistent_empty_dirs: bool,
    pub create_dir: bool,
    pub remove_entry: bool,
    pub rename: RenameSupport,
    pub permissions: AttributeSupport,
    pub xattrs: AttributeSupport,
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
    /// Check semantic consistency, without probing a backend or checking access.
    ///
    /// Both hierarchical and prefix models may support persistent empty directories
    /// and directory creation. Prefix adapters must define their marker semantics.
    /// Read-only handles may report persistent directories without creation support.
    pub fn validate(self) -> Result<(), StorageCapabilitiesError> {
        self.write_file.validate()?;
        if self.dir_model == DirModel::None {
            if self.persistent_empty_dirs {
                return Err(StorageCapabilitiesError::EmptyDirsWithoutDirModel);
            }
            if self.create_dir {
                return Err(StorageCapabilitiesError::CreateDirWithoutDirModel);
            }
        }
        Ok(())
    }

    /// Whether this declaration covers every capability in an opened instance.
    ///
    /// Invalid declarations never cover or are covered by another declaration.
    /// For valid declarations this is a partial order: reflexive, antisymmetric
    /// and transitive. `None` is the bottom directory model; hierarchical and
    /// prefix semantics are incomparable. Range reads cover sequential reads.
    /// Coverage does not prove that an adapter implements its advertised behavior.
    pub fn supports(self, actual: Self) -> bool {
        if self.validate().is_err() || actual.validate().is_err() {
            return false;
        }
        let read = matches!(
            (self.read_file, actual.read_file),
            (_, FileReadSupport::None)
                | (FileReadSupport::Range, _)
                | (FileReadSupport::Sequential, FileReadSupport::Sequential)
        );
        let attribute = |declared: AttributeSupport, actual: AttributeSupport| {
            (!actual.can_read() || declared.can_read())
                && (!actual.can_write() || declared.can_write())
        };
        (!actual.stat_entry || self.stat_entry)
            && (!actual.list_entries || self.list_entries)
            && read
            && (!actual.write_file.create_new || self.write_file.create_new)
            && (!actual.write_file.replace || self.write_file.replace)
            && (!actual.write_file.atomic_publish || self.write_file.atomic_publish)
            && (actual.dir_model == DirModel::None || actual.dir_model == self.dir_model)
            && (!actual.persistent_empty_dirs || self.persistent_empty_dirs)
            && (!actual.create_dir || self.create_dir)
            && (!actual.remove_entry || self.remove_entry)
            && (!actual.rename.no_replace || self.rename.no_replace)
            && (!actual.rename.replace || self.rename.replace)
            && attribute(self.permissions, actual.permissions)
            && attribute(self.xattrs, actual.xattrs)
    }
}
