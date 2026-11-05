//! Entry metadata whose shape excludes file-size fields on non-files.
use super::StoragePath;
use bon::{Builder, bon};
use getset::{CopyGetters, Getters};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EntryType {
    /// An adapter may omit a file's size when the backend cannot provide it.
    #[non_exhaustive]
    File {
        size: Option<u64>,
    },
    Directory,
    Symlink,
    Other,
}
#[bon]
impl EntryType {
    /// Construct file metadata; omitted size denotes an unknown size.
    #[builder(finish_fn = build, state_mod(vis = "pub"))]
    pub fn file(size: Option<u64>) -> Self {
        Self::File { size }
    }
}
/// Metadata describes the entry itself; stat never follows the final symlink.
/// Native timestamp encodings are converted at the adapter boundary. Metadata
/// from separate operations need not describe the same backend revision.
#[derive(Clone, Debug, Eq, PartialEq, Builder, Getters, CopyGetters)]
#[builder(state_mod(vis = "pub"))]
pub struct Entry {
    #[getset(get = "pub")]
    path: StoragePath,
    #[getset(get_copy = "pub")]
    entry_type: EntryType,
    #[getset(get_copy = "pub")]
    modified: Option<SystemTime>,
}
