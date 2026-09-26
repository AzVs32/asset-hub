use std::collections::BTreeMap;
use std::io::{Cursor, Error, Read};
use std::sync::{Arc, RwLock};

use asset_vfs::driver::{BoundDriver, Driver, DriverError, DriverPath};
use asset_vfs::entry::Entry;
use asset_vfs::error::VfsError;
use asset_vfs::namespace::{EntryName, VirtualPath, VirtualRelativePath};

#[derive(Clone)]
enum Node {
    Directory,
    File(Arc<[u8]>),
}

type Tree = Arc<RwLock<BTreeMap<String, Node>>>;

/// A mutable in-memory tree that provides read-only bound drivers.
///
/// Directories must be created before their children. Clones share the same
/// tree, while open readers retain a snapshot of the file they opened.
#[derive(Clone)]
pub struct MemoryDriver {
    tree: Tree,
}

impl MemoryDriver {
    /// Creates a driver with an empty root directory.
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert("/".to_owned(), Node::Directory);
        Self {
            tree: Arc::new(RwLock::new(nodes)),
        }
    }

    /// Creates a directory. Creating an existing directory succeeds.
    pub fn create_directory(&self, path: &VirtualPath) -> Result<(), VfsError> {
        let mut nodes = self.tree.write().map_err(lock_error)?;
        if let Some(node) = nodes.get(path.as_str()) {
            return match node {
                Node::Directory => Ok(()),
                Node::File(_) => Err(DriverError::NotDirectory.into()),
            };
        }

        ensure_parent_directory(&nodes, path)?;
        nodes.insert(path.as_str().to_owned(), Node::Directory);
        Ok(())
    }

    /// Adds or replaces a file, provided its parent directory exists.
    pub fn insert_file(
        &self,
        path: &VirtualPath,
        contents: impl Into<Vec<u8>>,
    ) -> Result<(), VfsError> {
        if path.is_root() {
            return Err(DriverError::IsDirectory.into());
        }

        let mut nodes = self.tree.write().map_err(lock_error)?;
        if matches!(nodes.get(path.as_str()), Some(Node::Directory)) {
            return Err(DriverError::IsDirectory.into());
        }
        ensure_parent_directory(&nodes, path)?;
        nodes.insert(
            path.as_str().to_owned(),
            Node::File(Arc::from(contents.into())),
        );
        Ok(())
    }
}

impl Default for MemoryDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl Driver for MemoryDriver {
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, VfsError> {
        let path = if root.as_str().is_empty() {
            VirtualPath::root()
        } else {
            VirtualPath::try_from(root.as_str()).map_err(|_| DriverError::InvalidPath)?
        };

        let nodes = self.tree.read().map_err(lock_error)?;
        match nodes.get(path.as_str()) {
            Some(Node::Directory) => Ok(Box::new(MemoryBoundDriver {
                tree: Arc::clone(&self.tree),
                root: path.as_str().to_owned(),
            })),
            Some(Node::File(_)) => Err(DriverError::NotDirectory.into()),
            None => Err(DriverError::NotFound.into()),
        }
    }
}

struct MemoryBoundDriver {
    tree: Tree,
    root: String,
}

impl MemoryBoundDriver {
    fn resolve(&self, path: &VirtualRelativePath) -> String {
        if path.is_empty() {
            self.root.clone()
        } else if self.root == "/" {
            format!("/{}", path.as_str())
        } else {
            format!("{}/{}", self.root, path.as_str())
        }
    }
}

impl BoundDriver for MemoryBoundDriver {
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, VfsError> {
        let directory = self.resolve(path);
        let nodes = self.tree.read().map_err(lock_error)?;
        match nodes.get(&directory) {
            Some(Node::Directory) => {}
            Some(Node::File(_)) => return Err(DriverError::NotDirectory.into()),
            None => return Err(DriverError::NotFound.into()),
        }

        let prefix = if directory == "/" {
            "/".to_owned()
        } else {
            format!("{directory}/")
        };
        let mut entries = Vec::new();
        for (name, node) in nodes.range(prefix.clone()..) {
            let Some(remainder) = name.strip_prefix(&prefix) else {
                break;
            };
            if remainder.contains('/') {
                continue;
            }

            let entry_name =
                EntryName::try_from(remainder).map_err(|_| DriverError::UnrepresentableName)?;
            entries.push(match node {
                Node::Directory => Entry::directory(entry_name),
                Node::File(contents) => Entry::file(entry_name, Some(contents.len() as u64)),
            });
        }
        Ok(entries)
    }

    fn read(&self, path: &VirtualRelativePath) -> Result<Box<dyn Read + Send + 'static>, VfsError> {
        let file = self.resolve(path);
        let nodes = self.tree.read().map_err(lock_error)?;
        match nodes.get(&file) {
            Some(Node::File(contents)) => Ok(Box::new(Cursor::new(Arc::clone(contents)))),
            Some(Node::Directory) => Err(DriverError::IsDirectory.into()),
            None => Err(DriverError::NotFound.into()),
        }
    }
}

fn ensure_parent_directory(
    nodes: &BTreeMap<String, Node>,
    path: &VirtualPath,
) -> Result<(), DriverError> {
    let parent = path.parent().ok_or(DriverError::IsDirectory)?;
    match nodes.get(parent.as_str()) {
        Some(Node::Directory) => Ok(()),
        Some(Node::File(_)) => Err(DriverError::NotDirectory),
        None => Err(DriverError::NotFound),
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> DriverError {
    DriverError::backend(Error::other("memory driver lock poisoned"))
}
