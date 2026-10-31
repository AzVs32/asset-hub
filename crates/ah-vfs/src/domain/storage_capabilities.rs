//! Transport-independent storage capabilities and semantic guarantees.

/// Prefix dirs do not imply persistent empty dirs. Adapters must document marker
/// creation and file/prefix collisions instead of silently pretending POSIX dirs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DirModel {
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

impl StorageCapabilities {
    /// Whether this declaration covers every capability in an opened instance.
    pub fn supports(self, actual: Self) -> bool {
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
