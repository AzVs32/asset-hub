//! Infallible capability composition: contradictory declarations have no representation.
use super::{
    Operation, Publication, ReadRange, ReadRequest, Rejection, RenameMode, RenameRequest,
    WriteMode, WriteRequest,
};
use bon::{Builder, bon};
use getset::CopyGetters;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
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
#[non_exhaustive]
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
#[bon]
impl WriteSupport {
    /// Declare staged writes with an explicit, nonempty set of supported modes.
    #[builder(finish_fn = build, state_mod(vis = "pub"))]
    pub fn staged(modes: WriteModes, #[builder(default)] atomic_publish: bool) -> Self {
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
#[bon]
impl DirectorySupport {
    /// Declare a hierarchy; each optional guarantee defaults to unsupported.
    #[builder(finish_fn = build, state_mod(vis = "pub"))]
    pub fn hierarchical(
        #[builder(default)] create: bool,
        #[builder(default)] persistent_empty: bool,
    ) -> Self {
        Self::Hierarchical {
            create,
            persistent_empty,
        }
    }
    /// Declare prefix directories without assuming marker or empty-directory support.
    #[builder(finish_fn = build, state_mod(vis = "pub"))]
    pub fn prefix(
        #[builder(default)] create: bool,
        #[builder(default)] persistent_empty: bool,
    ) -> Self {
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
#[non_exhaustive]
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
#[non_exhaustive]
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

/// Driver-wide operation and guarantee declarations, shared by all instances.
/// Composition is infallible because each supported mode carries its prerequisites.
/// The builder needs no additional validate step. Backend/path/authorization restrictions
/// remain operation failures, and declarations do not prove implementation correctness.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Builder, CopyGetters)]
#[builder(state_mod(vis = "pub"))]
#[getset(get_copy = "pub")]
pub struct Capabilities {
    #[builder(default)]
    stat: bool,
    #[builder(default)]
    list: bool,
    #[builder(default)]
    remove: bool,
    #[builder(default)]
    read: ReadSupport,
    #[builder(default)]
    write: WriteSupport,
    #[builder(default)]
    directories: DirectorySupport,
    #[builder(default)]
    rename: RenameSupport,
    #[builder(default)]
    permissions: AttributeSupport,
    #[builder(default)]
    xattrs: AttributeSupport,
}
impl Capabilities {
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
