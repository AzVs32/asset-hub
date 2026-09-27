//! Shared storage-independent data for driver conformance fixtures.
//!
//! The standard trees below are the source of truth for both native setup and
//! expected listings. Implement [`TreeBuilder`] once per storage backend, then
//! use [`TreeSpec::populate`] from each fixture. This is test setup, independent
//! of the production `WriteDriver` capability. Pre-provisioned read-only storage
//! may expose the same data without using a builder.

use asset_vfs::entry::{Entry, EntryKind};
use asset_vfs::namespace::{EntryName, VirtualRelativePath};

use super::support::relative;

/// One entry with a canonical, nonempty path relative to a fixture root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeEntry {
    Directory(&'static str),
    File {
        path: &'static str,
        contents: &'static [u8],
    },
}

impl TreeEntry {
    fn raw_path(&self) -> &'static str {
        match self {
            Self::Directory(path) | Self::File { path, .. } => path,
        }
    }

    /// Returns the entry's validated relative path.
    pub fn path(&self) -> VirtualRelativePath {
        let raw = self.raw_path();
        assert!(
            !raw.is_empty(),
            "tree entries must not describe the implicit root"
        );
        relative(&format!("/{raw}"))
    }

    /// Returns the expected entry kind.
    pub fn kind(&self) -> EntryKind {
        match self {
            Self::Directory(_) => EntryKind::Directory,
            Self::File { .. } => EntryKind::File,
        }
    }
}

/// A tree whose root directory is implicit.
///
/// Declare each ancestor directory explicitly. Paths must be canonical and
/// unique. Declaration order does not affect population or expected ordering.
#[derive(Debug, Clone, Copy)]
pub struct TreeSpec {
    entries: &'static [TreeEntry],
}

impl TreeSpec {
    pub const fn new(entries: &'static [TreeEntry]) -> Self {
        Self { entries }
    }

    pub const fn entries(&self) -> &'static [TreeEntry] {
        self.entries
    }

    /// Creates the root, directories in parent-first order, then files.
    ///
    /// Returns the builder's first error unchanged and stops population.
    pub fn populate<B: TreeBuilder + ?Sized>(&self, builder: &mut B) -> Result<(), B::Error> {
        builder.create_directory(&root_path())?;
        let mut directories: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.kind() == EntryKind::Directory)
            .map(TreeEntry::path)
            .collect();
        directories.sort_by_key(VirtualRelativePath::depth);
        for path in directories {
            builder.create_directory(&path)?;
        }
        for entry in self.entries {
            if let TreeEntry::File { contents, .. } = entry {
                builder.write_file(&entry.path(), contents)?;
            }
        }
        Ok(())
    }

    /// Returns the expected direct children of a directory, sorted by name.
    ///
    /// File sizes are derived from the described bytes. Checks still allow the
    /// actual reader to report unknown sizes. This describes data; it does not
    /// perform storage access or report errors for absent or non-directory paths.
    pub fn listing(&self, path: &VirtualRelativePath) -> Vec<Entry> {
        let mut entries = Vec::new();
        for entry in self.entries {
            let (parent, name) = entry
                .raw_path()
                .rsplit_once('/')
                .unwrap_or(("", entry.raw_path()));
            if parent != path.as_str() {
                continue;
            }
            let name = EntryName::try_from(name).expect("tree entry names must be valid");
            entries.push(match entry {
                TreeEntry::Directory(_) => Entry::directory(name),
                TreeEntry::File { contents, .. } => Entry::file(name, Some(contents.len() as u64)),
            });
        }
        entries.sort_unstable_by(|left, right| left.name().cmp(right.name()));
        entries
    }
}

/// Native storage operations used only to prepare conformance data.
///
/// Map every path to the builder's chosen root. Creating an existing directory
/// should succeed. File writes create or replace the described file. Population
/// orders directories before children, so a builder need not create parents.
pub trait TreeBuilder {
    type Error;

    fn create_directory(&mut self, path: &VirtualRelativePath) -> Result<(), Self::Error>;
    fn write_file(
        &mut self,
        path: &VirtualRelativePath,
        contents: &[u8],
    ) -> Result<(), Self::Error>;
}

/// The empty relative path addressing a fixture's root directory.
pub fn root_path() -> VirtualRelativePath {
    relative("/")
}

/// An absent child used for missing-path and file-ancestor checks.
///
/// Standard trees reserve this name beneath the root and every directory.
pub fn missing_child(parent: &VirtualRelativePath) -> VirtualRelativePath {
    if parent.is_empty() {
        relative("/missing")
    } else {
        relative(&format!("/{}/missing", parent.as_str()))
    }
}

/// A directory containing a file, sufficient for root-binding checks.
pub const BINDING_TREE: TreeSpec = TreeSpec::new(&[TreeEntry::File {
    path: "file.txt",
    contents: b"",
}]);

/// The first of two distinguishable roots for bound-backend checks.
pub const BOUND_ROOT_TREE: TreeSpec = TreeSpec::new(&[TreeEntry::File {
    path: "root.txt",
    contents: b"root",
}]);

/// The other root, with names disjoint from the first root.
pub const BOUND_OTHER_TREE: TreeSpec = TreeSpec::new(&[TreeEntry::File {
    path: "other.txt",
    contents: b"other",
}]);

/// Read data covering direct children, nested directories, empty directories,
/// empty and nonempty files, and Unicode names. File sizes come from these bytes.
pub const READ_TREE: TreeSpec = TreeSpec::new(&[
    TreeEntry::Directory("2026"),
    TreeEntry::File {
        path: "2026/nested.txt",
        contents: b"nested",
    },
    TreeEntry::File {
        path: "a.txt",
        contents: b"",
    },
    TreeEntry::Directory("empty"),
    TreeEntry::File {
        path: "z.txt",
        contents: b"hello",
    },
    TreeEntry::Directory("子目录"),
    TreeEntry::File {
        path: "子目录/文件.txt",
        contents: b"unicode",
    },
]);
