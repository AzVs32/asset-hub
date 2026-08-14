//! Directory aggregate application service.

mod action;
mod command;
mod contract;
mod secured;

pub(crate) use contract::ExecutedDirectoryAction;
pub use contract::{DirectoryActions, ExecuteDirectoryAction, UpdateDirectory};
pub use secured::SecuredDirectoryService;

use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryKind, DirectoryKindDefinition, DirectoryPath},
    port::{
        DirectoryActionExecutor, DirectoryActionRegistry, DirectoryIndex, DirectoryKindRegistry,
        DirectoryLocation, DirectoryRepository, DirectoryStorage, LocatedDirectory,
    },
};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
struct DirectoryActionPorts {
    registry: Arc<dyn DirectoryActionRegistry>,
    executor: Arc<dyn DirectoryActionExecutor>,
}

/// Coordinates directory aggregates, the durable store, the query index, and physical storage.
#[derive(Clone)]
pub struct DirectoryService {
    repository: Arc<dyn DirectoryRepository>,
    index: Arc<dyn DirectoryIndex>,
    storage: Arc<dyn DirectoryStorage>,
    kind_registry: Arc<dyn DirectoryKindRegistry>,
    mutation_lock: Arc<Mutex<()>>,
    action_ports: Option<DirectoryActionPorts>,
}

impl DirectoryService {
    pub fn secured<'a>(
        &'a self,
        authorization: &'a crate::service::AuthorizationService,
        context: &'a crate::domain::AccessContext,
    ) -> SecuredDirectoryService<'a> {
        SecuredDirectoryService::new(self, authorization, context)
    }

    pub fn new(
        repository: Arc<dyn DirectoryRepository>,
        index: Arc<dyn DirectoryIndex>,
        storage: Arc<dyn DirectoryStorage>,
        kind_registry: Arc<dyn DirectoryKindRegistry>,
    ) -> Self {
        Self {
            repository,
            index,
            storage,
            kind_registry,
            mutation_lock: Arc::new(Mutex::new(())),
            action_ports: None,
        }
    }

    pub fn kind_definitions(&self) -> &[DirectoryKindDefinition] {
        self.kind_registry.definitions()
    }

    pub fn kind_lineage(&self, kind: &DirectoryKind) -> Vec<DirectoryKind> {
        self.kind_registry.lineage(kind)
    }

    pub async fn reload_index(&self) -> Result<(), CoreError> {
        let directories = self.repository.load_all().await?;
        self.index.replace_all(directories).await
    }

    pub async fn root(&self) -> Result<DirectoryLocation, CoreError> {
        Ok(self
            .find_by_id(&DirectoryId::root())
            .await?
            .location()
            .clone())
    }

    pub async fn find_by_id(&self, id: &DirectoryId) -> Result<LocatedDirectory, CoreError> {
        self.index
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))
    }

    pub async fn locate_by_id(&self, id: &DirectoryId) -> Result<DirectoryLocation, CoreError> {
        Ok(self.find_by_id(id).await?.location().clone())
    }

    pub async fn find_by_path(&self, path: &DirectoryPath) -> Result<LocatedDirectory, CoreError> {
        self.index
            .find_by_path(path)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", path.path()))
    }

    pub async fn resolve_path(&self, path: &DirectoryPath) -> Result<DirectoryLocation, CoreError> {
        Ok(self.find_by_path(path).await?.location().clone())
    }

    pub async fn list_children(
        &self,
        parent: &DirectoryLocation,
    ) -> Result<Vec<DirectoryLocation>, CoreError> {
        Ok(self
            .list_located_children(&parent.id())
            .await?
            .into_iter()
            .map(|directory| directory.location().clone())
            .collect())
    }

    pub async fn list_located_children(
        &self,
        parent_id: &DirectoryId,
    ) -> Result<Vec<LocatedDirectory>, CoreError> {
        self.index.list_children(parent_id).await
    }

    pub async fn contains(
        &self,
        ancestor: &DirectoryId,
        candidate: &DirectoryId,
    ) -> Result<bool, CoreError> {
        self.index.is_descendant_or_self(ancestor, candidate).await
    }

    fn ensure_kind_registered(&self, kind: &DirectoryKind) -> Result<(), CoreError> {
        if self.kind_registry.supports(kind) {
            Ok(())
        } else {
            Err(CoreError::unsupported("directory kind", kind.to_string()))
        }
    }

    fn kind_for_new_child(
        &self,
        parent_kind: &DirectoryKind,
        requested_kind: DirectoryKind,
    ) -> DirectoryKind {
        if requested_kind == DirectoryKind::default() {
            self.kind_registry
                .get(parent_kind)
                .and_then(DirectoryKindDefinition::default_child_kind)
                .cloned()
                .unwrap_or(requested_kind)
        } else {
            requested_kind
        }
    }

    fn ensure_parent_kind_allowed(
        &self,
        child_kind: &DirectoryKind,
        parent_kind: &DirectoryKind,
    ) -> Result<(), CoreError> {
        let allowed = self
            .kind_registry
            .lineage(child_kind)
            .into_iter()
            .find_map(|kind| {
                let declared = self
                    .kind_registry
                    .get(&kind)
                    .expect("registered kind lineage must contain definitions")
                    .allowed_parent_kinds();
                (!declared.is_empty()).then(|| declared.to_vec())
            })
            .unwrap_or_default();
        if allowed.is_empty()
            || allowed
                .iter()
                .any(|kind| self.kind_registry.is_a(parent_kind, kind))
        {
            return Ok(());
        }
        Err(CoreError::conflict(format!(
            "directory kind `{child_kind}` does not allow parent kind `{parent_kind}`"
        )))
    }

    fn require_kind_registered(&self, kind: &DirectoryKind) -> Result<(), CoreError> {
        if self.kind_registry.supports(kind) {
            Ok(())
        } else {
            Err(CoreError::invariant(format!(
                "persisted directory kind `{kind}` is not registered"
            )))
        }
    }

    async fn update_index(&self, directory: Directory) -> Result<(), CoreError> {
        if let Err(error) = self.index.upsert(directory).await {
            let _ = self.reload_index().await;
            return Err(error);
        }
        Ok(())
    }
}
