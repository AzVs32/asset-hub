use std::collections::BTreeMap;
use std::io::{Cursor, Error, Read};
use std::sync::{Arc, RwLock};

use asset_core::domain::Entry;
use asset_core::domain::{DriverPath, EntryName, VirtualPath, VirtualRelativePath};
use asset_core::error::CoreError;
use asset_core::port::{BoundDriver, Driver};

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
    pub fn create_directory(&self, path: &VirtualPath) -> Result<(), CoreError> {
        let mut nodes = self.tree.write().map_err(lock_error)?;
        if let Some(node) = nodes.get(path.as_str()) {
            return match node {
                Node::Directory => Ok(()),
                Node::File(_) => Err(CoreError::driver_not_directory()),
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
    ) -> Result<(), CoreError> {
        if path.is_root() {
            return Err(CoreError::driver_is_directory());
        }

        let mut nodes = self.tree.write().map_err(lock_error)?;
        if matches!(nodes.get(path.as_str()), Some(Node::Directory)) {
            return Err(CoreError::driver_is_directory());
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
    fn bind(&self, root: &DriverPath) -> Result<Box<dyn BoundDriver>, CoreError> {
        let path = if root.as_str().is_empty() {
            VirtualPath::root()
        } else {
            VirtualPath::try_from(root.as_str()).map_err(|_| CoreError::driver_invalid_path())?
        };

        let nodes = self.tree.read().map_err(lock_error)?;
        match nodes.get(path.as_str()) {
            Some(Node::Directory) => Ok(Box::new(MemoryBoundDriver {
                tree: Arc::clone(&self.tree),
                root: path.as_str().to_owned(),
            })),
            Some(Node::File(_)) => Err(CoreError::driver_not_directory()),
            None => Err(CoreError::driver_not_found()),
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
    fn list(&self, path: &VirtualRelativePath) -> Result<Vec<Entry>, CoreError> {
        let directory = self.resolve(path);
        let nodes = self.tree.read().map_err(lock_error)?;
        match nodes.get(&directory) {
            Some(Node::Directory) => {}
            Some(Node::File(_)) => return Err(CoreError::driver_not_directory()),
            None => return Err(CoreError::driver_not_found()),
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

            let entry_name = EntryName::try_from(remainder)
                .map_err(|_| CoreError::driver_unrepresentable_name())?;
            entries.push(match node {
                Node::Directory => Entry::directory(entry_name),
                Node::File(contents) => Entry::file(entry_name, Some(contents.len() as u64)),
            });
        }
        Ok(entries)
    }

    fn read(
        &self,
        path: &VirtualRelativePath,
    ) -> Result<Box<dyn Read + Send + 'static>, CoreError> {
        let file = self.resolve(path);
        let nodes = self.tree.read().map_err(lock_error)?;
        match nodes.get(&file) {
            Some(Node::File(contents)) => Ok(Box::new(Cursor::new(Arc::clone(contents)))),
            Some(Node::Directory) => Err(CoreError::driver_is_directory()),
            None => Err(CoreError::driver_not_found()),
        }
    }
}

fn ensure_parent_directory(
    nodes: &BTreeMap<String, Node>,
    path: &VirtualPath,
) -> Result<(), CoreError> {
    let parent = path.parent().ok_or(CoreError::driver_is_directory())?;
    match nodes.get(parent.as_str()) {
        Some(Node::Directory) => Ok(()),
        Some(Node::File(_)) => Err(CoreError::driver_not_directory()),
        None => Err(CoreError::driver_not_found()),
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> CoreError {
    CoreError::backend(Error::other("memory driver lock poisoned"))
}
