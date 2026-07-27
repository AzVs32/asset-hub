use asset_core::CoreError;
use asset_core::domain::{Directory, DirectoryId, DirectoryPath};
use asset_core::port::{DirectoryIndex, DirectoryLocation, DirectoryQuery, LocatedDirectory};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::RwLock;

/// Process-local, rebuildable projection of the complete directory tree.
#[derive(Debug, Default)]
pub(crate) struct InMemoryDirectoryIndex {
    state: RwLock<DirectoryIndexState>,
}

#[derive(Debug, Clone, Default)]
struct DirectoryIndexState {
    nodes: HashMap<DirectoryId, Directory>,
    children: HashMap<DirectoryId, BTreeMap<String, DirectoryId>>,
}

impl InMemoryDirectoryIndex {
    pub(crate) fn from_directories(directories: Vec<Directory>) -> Result<Self, CoreError> {
        Ok(Self {
            state: RwLock::new(build_state(directories)?),
        })
    }

    fn state(&self) -> Result<std::sync::RwLockReadGuard<'_, DirectoryIndexState>, CoreError> {
        self.state
            .read()
            .map_err(|_| CoreError::configuration("directory index read lock is poisoned"))
    }

    fn state_mut(&self) -> Result<std::sync::RwLockWriteGuard<'_, DirectoryIndexState>, CoreError> {
        self.state
            .write()
            .map_err(|_| CoreError::configuration("directory index write lock is poisoned"))
    }
}

#[async_trait::async_trait]
impl DirectoryQuery for InMemoryDirectoryIndex {
    async fn find_by_id(&self, id: &DirectoryId) -> Result<Option<LocatedDirectory>, CoreError> {
        let state = self.state()?;
        state
            .nodes
            .get(id)
            .map(|directory| located(&state, directory))
            .transpose()
    }

    async fn find_by_path(
        &self,
        path: &DirectoryPath,
    ) -> Result<Option<LocatedDirectory>, CoreError> {
        let state = self.state()?;
        let mut id = DirectoryId::root();
        for name in path.path().split('/').filter(|name| !name.is_empty()) {
            let Some(child) = state
                .children
                .get(&id)
                .and_then(|children| children.get(name))
            else {
                return Ok(None);
            };
            id = *child;
        }
        state
            .nodes
            .get(&id)
            .map(|directory| located(&state, directory))
            .transpose()
    }

    async fn list_children(
        &self,
        parent_id: &DirectoryId,
    ) -> Result<Vec<LocatedDirectory>, CoreError> {
        let state = self.state()?;
        if !state.nodes.contains_key(parent_id) {
            return Ok(Vec::new());
        }
        state
            .children
            .get(parent_id)
            .into_iter()
            .flat_map(BTreeMap::values)
            .map(|id| {
                let directory = state.nodes.get(id).ok_or_else(|| {
                    CoreError::configuration("directory index child references a missing node")
                })?;
                located(&state, directory)
            })
            .collect()
    }

    async fn is_descendant_or_self(
        &self,
        ancestor_id: &DirectoryId,
        candidate_id: &DirectoryId,
    ) -> Result<bool, CoreError> {
        let state = self.state()?;
        let mut current = Some(*candidate_id);
        let mut visited = HashSet::new();
        while let Some(id) = current {
            if id == *ancestor_id {
                return Ok(true);
            }
            if !visited.insert(id) {
                return Err(CoreError::configuration(
                    "directory index contains a parent cycle",
                ));
            }
            current = state.nodes.get(&id).and_then(Directory::parent_id);
        }
        Ok(false)
    }
}

#[async_trait::async_trait]
impl DirectoryIndex for InMemoryDirectoryIndex {
    async fn replace_all(&self, directories: Vec<Directory>) -> Result<(), CoreError> {
        *self.state_mut()? = build_state(directories)?;
        Ok(())
    }

    async fn upsert(&self, directory: Directory) -> Result<(), CoreError> {
        let mut state = self.state_mut()?;
        let mut directories = state.nodes.values().cloned().collect::<Vec<_>>();
        if let Some(existing) = directories
            .iter_mut()
            .find(|existing| existing.id() == directory.id())
        {
            *existing = directory;
        } else {
            directories.push(directory);
        }
        *state = build_state(directories)?;
        Ok(())
    }

    async fn remove(&self, id: &DirectoryId) -> Result<(), CoreError> {
        if id.is_root() {
            return Err(CoreError::configuration(
                "root directory cannot be removed from the index",
            ));
        }
        let mut state = self.state_mut()?;
        if state
            .children
            .get(id)
            .is_some_and(|children| !children.is_empty())
        {
            return Err(CoreError::conflict(
                "directory with children cannot be removed from the index",
            ));
        }
        let directories = state
            .nodes
            .values()
            .filter(|directory| directory.id() != *id)
            .cloned()
            .collect();
        *state = build_state(directories)?;
        Ok(())
    }
}

fn build_state(directories: Vec<Directory>) -> Result<DirectoryIndexState, CoreError> {
    let mut nodes = HashMap::with_capacity(directories.len());
    for directory in directories {
        if nodes.insert(directory.id(), directory).is_some() {
            return Err(CoreError::configuration(
                "directory store returned duplicate IDs",
            ));
        }
    }
    let Some(root) = nodes.get(&DirectoryId::root()) else {
        return Err(CoreError::configuration("root directory is missing"));
    };
    if root.parent_id().is_some() || !root.name().is_empty() {
        return Err(CoreError::configuration("root directory is invalid"));
    }

    let mut children = HashMap::<DirectoryId, BTreeMap<String, DirectoryId>>::new();
    for directory in nodes.values().filter(|directory| !directory.id().is_root()) {
        let parent_id = directory
            .parent_id()
            .ok_or_else(|| CoreError::configuration("non-root directory is missing its parent"))?;
        if !nodes.contains_key(&parent_id) {
            return Err(CoreError::configuration(format!(
                "directory `{}` references missing parent `{parent_id}`",
                directory.id()
            )));
        }
        if children
            .entry(parent_id)
            .or_default()
            .insert(directory.name().to_owned(), directory.id())
            .is_some()
        {
            return Err(CoreError::configuration(
                "directory index contains duplicate sibling names",
            ));
        }
    }
    let state = DirectoryIndexState { nodes, children };
    for id in state.nodes.keys() {
        path_for(&state, id)?;
    }
    Ok(state)
}

fn located(
    state: &DirectoryIndexState,
    directory: &Directory,
) -> Result<LocatedDirectory, CoreError> {
    LocatedDirectory::new(
        directory.clone(),
        DirectoryLocation::new(directory.id(), path_for(state, &directory.id())?),
    )
}

fn path_for(
    state: &DirectoryIndexState,
    directory_id: &DirectoryId,
) -> Result<DirectoryPath, CoreError> {
    let mut names = Vec::new();
    let mut current = Some(*directory_id);
    let mut visited = HashSet::new();
    while let Some(id) = current {
        if !visited.insert(id) {
            return Err(CoreError::configuration(
                "directory index contains a parent cycle",
            ));
        }
        let directory = state.nodes.get(&id).ok_or_else(|| {
            CoreError::configuration(format!("directory index is missing node `{id}`"))
        })?;
        if !id.is_root() {
            names.push(directory.name());
        }
        current = directory.parent_id();
    }
    names.reverse();
    DirectoryPath::from_path(names.join("/")).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn index_recomputes_descendant_paths_after_parent_updates() {
        let root = Directory::root();
        let mut parent = Directory::new(root.id(), "parent").unwrap();
        let child = Directory::new(parent.id(), "child").unwrap();
        let index =
            InMemoryDirectoryIndex::from_directories(vec![root, parent.clone(), child.clone()])
                .unwrap();

        parent.rename("renamed").unwrap();
        index.upsert(parent).await.unwrap();

        assert_eq!(
            index
                .find_by_id(&child.id())
                .await
                .unwrap()
                .unwrap()
                .path()
                .path(),
            "renamed/child"
        );
    }
}
