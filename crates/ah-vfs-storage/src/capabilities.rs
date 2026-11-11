//! Infallible capability composition: contradictory declarations have no representation.
use super::{
    Operation, Publication, ReadRange, ReadRequest, Rejection, RenameMode, RenameRequest,
    WriteMode, WriteRequest,
};
use getset::CopyGetters;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReadSupport {
    #[default]
    None,
    /// Offset-zero reads, including bounded prefixes. Arbitrary offsets require Range.
    Sequential,
    Range,
}
impl ReadSupport {
    pub fn supports(self, range: ReadRange) -> bool {
        match self {
            Self::None => false,
            Self::Sequential => range.offset() == 0,
            Self::Range => true,
        }
    }
}
/// A nonempty set of write modes. There is no empty mode set to contradict
/// an atomic-publication promise; absence of writing is WriteSupport::None.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteModes {
    CreateNew,
    Replace,
    Both,
}
impl WriteModes {
    pub fn supports(self, mode: WriteMode) -> bool {
        // Enumerate both axes: adding a mode or declaration must force a review.
        match (self, mode) {
            (Self::CreateNew | Self::Both, WriteMode::CreateNew)
            | (Self::Replace | Self::Both, WriteMode::Replace) => true,
            (Self::CreateNew, WriteMode::Replace) | (Self::Replace, WriteMode::CreateNew) => false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum WriteSupport {
    #[default]
    None,
    /// All writes stage until commit. Atomic visibility never implies crash durability.
    #[non_exhaustive]
    Staged {
        modes: WriteModes,
        atomic_publish: bool,
    },
}
impl WriteSupport {
    /// Declare staged writes with an explicit, nonempty set of supported modes.
    pub fn staged(modes: WriteModes, atomic_publish: bool) -> Self {
        Self::Staged {
            modes,
            atomic_publish,
        }
    }
    pub fn supports(self, mode: WriteMode, publication: Publication) -> bool {
        // A new guarantee cannot inherit atomic support merely by being non-Staged.
        let requires_atomic = match publication {
            Publication::Staged => false,
            Publication::Atomic => true,
        };
        match self {
            Self::None => false,
            Self::Staged {
                modes,
                atomic_publish,
            } => modes.supports(mode) && (!requires_atomic || atomic_publish),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum DirectorySupport {
    #[default]
    None,
    #[non_exhaustive]
    Hierarchical {
        create: bool,
        persistent_empty: bool,
    },
    /// Markers and file/prefix collisions are adapter-specific and must be documented.
    #[non_exhaustive]
    Prefix {
        create: bool,
        persistent_empty: bool,
    },
}
impl DirectorySupport {
    /// Declare a hierarchy with explicit creation and empty-directory guarantees.
    pub fn hierarchical(create: bool, persistent_empty: bool) -> Self {
        Self::Hierarchical {
            create,
            persistent_empty,
        }
    }
    /// Declare prefix directories without assuming marker or empty-directory support.
    pub fn prefix(create: bool, persistent_empty: bool) -> Self {
        Self::Prefix {
            create,
            persistent_empty,
        }
    }
    pub fn can_create(self) -> bool {
        match self {
            Self::None => false,
            Self::Hierarchical { create, .. } | Self::Prefix { create, .. } => create,
        }
    }
    pub fn persistent_empty(self) -> bool {
        match self {
            Self::None => false,
            Self::Hierarchical {
                persistent_empty, ..
            }
            | Self::Prefix {
                persistent_empty, ..
            } => persistent_empty,
        }
    }
}
/// All supported renames are atomic within one instance, never copy/delete.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RenameSupport {
    #[default]
    None,
    NoReplace,
    Replace,
    Both,
}
impl RenameSupport {
    pub fn supports(self, mode: RenameMode) -> bool {
        match (self, mode) {
            (Self::NoReplace | Self::Both, RenameMode::NoReplace)
            | (Self::Replace | Self::Both, RenameMode::Replace) => true,
            (Self::None, RenameMode::NoReplace | RenameMode::Replace)
            | (Self::NoReplace, RenameMode::Replace)
            | (Self::Replace, RenameMode::NoReplace) => false,
        }
    }
}
/// Attribute writing always includes reading; authorization is checked per call.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AttributeSupport {
    #[default]
    None,
    Read,
    ReadWrite,
}
impl AttributeSupport {
    pub fn can_read(self) -> bool {
        match self {
            Self::None => false,
            Self::Read | Self::ReadWrite => true,
        }
    }
    pub fn can_write(self) -> bool {
        match self {
            Self::None | Self::Read => false,
            Self::ReadWrite => true,
        }
    }
}

/// The guarantee against replacement of the final name before a mutation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum MutationBinding {
    /// The parent is fixed and the final slot is checked before the mutation.
    /// Another writer can replace that slot between the check and the effect.
    #[default]
    ParentPinned,
    /// The final slot is checked atomically with the effect. A mismatch is
    /// rejected as [`Rejection::Conflict`] before any effect occurs.
    FinalSlotAtomic,
}

/// Driver-wide operation and guarantee declarations, shared by all instances.
/// Composition is infallible because each supported mode carries its prerequisites.
/// The default declaration supports no operations. Each `with_*` method
/// updates one capability. Backend/path/authorization restrictions
/// remain operation failures, and declarations do not prove implementation correctness.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, CopyGetters)]
#[getset(get_copy = "pub")]
pub struct Capabilities {
    stat: bool,
    list: bool,
    remove: bool,
    read: ReadSupport,
    /// Reading a link's stored target is independent of following it in lookup.
    read_link: bool,
    write: WriteSupport,
    directories: DirectorySupport,
    rename: RenameSupport,
    permissions: AttributeSupport,
    xattrs: AttributeSupport,
    mutation_binding: MutationBinding,
}
impl Capabilities {
    pub fn with_stat(mut self, stat: bool) -> Self {
        self.stat = stat;
        self
    }

    pub fn with_list(mut self, list: bool) -> Self {
        self.list = list;
        self
    }

    pub fn with_remove(mut self, remove: bool) -> Self {
        self.remove = remove;
        self
    }

    pub fn with_read(mut self, read: ReadSupport) -> Self {
        self.read = read;
        self
    }

    pub fn with_read_link(mut self, read_link: bool) -> Self {
        self.read_link = read_link;
        self
    }

    pub fn with_write(mut self, write: WriteSupport) -> Self {
        self.write = write;
        self
    }

    pub fn with_directories(mut self, directories: DirectorySupport) -> Self {
        self.directories = directories;
        self
    }

    pub fn with_rename(mut self, rename: RenameSupport) -> Self {
        self.rename = rename;
        self
    }

    pub fn with_permissions(mut self, permissions: AttributeSupport) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn with_xattrs(mut self, xattrs: AttributeSupport) -> Self {
        self.xattrs = xattrs;
        self
    }

    pub fn with_mutation_binding(mut self, mutation_binding: MutationBinding) -> Self {
        self.mutation_binding = mutation_binding;
        self
    }

    /// Coarse operation admission; read/write/rename requests must additionally
    /// check their requested modes and guarantees with the require_* methods.
    pub fn supports(self, operation: Operation) -> bool {
        match operation {
            Operation::Stat => self.stat,
            Operation::List => self.list,
            Operation::Remove => self.remove,
            Operation::Read => match self.read {
                ReadSupport::None => false,
                ReadSupport::Sequential | ReadSupport::Range => true,
            },
            Operation::ReadLink => self.read_link,
            Operation::Write => match self.write {
                WriteSupport::None => false,
                WriteSupport::Staged { .. } => true,
            },
            Operation::CreateDirectory => self.directories.can_create(),
            Operation::Rename => match self.rename {
                RenameSupport::None => false,
                RenameSupport::NoReplace | RenameSupport::Replace | RenameSupport::Both => true,
            },
            Operation::GetPermissions => self.permissions.can_read(),
            Operation::SetPermissions => self.permissions.can_write(),
            Operation::ListXattrs | Operation::GetXattr => self.xattrs.can_read(),
            Operation::SetXattr | Operation::RemoveXattr => self.xattrs.can_write(),
        }
    }
    pub fn require_read(self, request: &ReadRequest) -> Result<(), Rejection> {
        require(self.read.supports(request.range()))
    }
    pub fn require_write(self, request: &WriteRequest) -> Result<(), Rejection> {
        require(self.write.supports(request.mode(), request.publication()))
    }
    pub fn require_rename(self, request: &RenameRequest) -> Result<(), Rejection> {
        require(self.rename.supports(request.mode()))
    }
}
fn require(supported: bool) -> Result<(), Rejection> {
    if supported {
        Ok(())
    } else {
        Err(Rejection::Unsupported)
    }
}
