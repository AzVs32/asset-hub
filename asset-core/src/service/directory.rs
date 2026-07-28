//! Directory aggregate application service.

use crate::{
    CoreError,
    domain::{Directory, DirectoryId, DirectoryKind, DirectoryPath},
    port::{
        DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRegistry,
        DirectoryActionRequest, DirectoryIndex, DirectoryKindRegistry, DirectoryLocation,
        DirectoryStorage, DirectoryStore, LocatedDirectory,
    },
};
use asset_plugin_api::protocol::directory::DirectoryActionEffect;
use asset_plugin_api::{DirectoryAction, DirectoryActionAccess, DirectoryActionDefinition};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A partial update to a directory aggregate.
#[derive(Debug, Clone, Default)]
pub struct UpdateDirectory {
    name: Option<String>,
    parent_id: Option<DirectoryId>,
    kind: Option<DirectoryKind>,
}

impl UpdateDirectory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_parent_id(mut self, parent_id: DirectoryId) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    pub fn with_kind(mut self, kind: DirectoryKind) -> Self {
        self.kind = Some(kind);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ExecuteDirectoryAction {
    pub action: DirectoryAction,
    pub input: Value,
}

impl ExecuteDirectoryAction {
    pub fn new(action: impl Into<DirectoryAction>) -> Self {
        Self {
            action: action.into(),
            input: Value::Null,
        }
    }

    pub fn with_input(mut self, input: Value) -> Self {
        self.input = input;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct DirectoryActions {
    available_actions: Vec<DirectoryActionDefinition>,
}

pub(crate) struct ExecutedDirectoryAction {
    directory_id: DirectoryId,
    expected_updated_at: DateTime<Utc>,
    access: DirectoryActionAccess,
    output: DirectoryActionOutput,
}

impl ExecutedDirectoryAction {
    pub(crate) fn into_output(self) -> DirectoryActionOutput {
        self.output
    }
}

impl DirectoryActions {
    fn new(available_actions: Vec<DirectoryActionDefinition>) -> Self {
        Self { available_actions }
    }
    pub fn available_actions(&self) -> &[DirectoryActionDefinition] {
        &self.available_actions
    }
}

#[derive(Clone)]
struct DirectoryActionPorts {
    registry: Arc<dyn DirectoryActionRegistry>,
    executor: Arc<dyn DirectoryActionExecutor>,
}

/// Coordinates directory aggregates, the durable store, the query index, and physical storage.
#[derive(Clone)]
pub struct DirectoryService {
    store: Arc<dyn DirectoryStore>,
    index: Arc<dyn DirectoryIndex>,
    storage: Arc<dyn DirectoryStorage>,
    kind_registry: Arc<dyn DirectoryKindRegistry>,
    mutation_lock: Arc<Mutex<()>>,
    action_ports: Option<DirectoryActionPorts>,
}

impl DirectoryService {
    pub fn new(
        store: Arc<dyn DirectoryStore>,
        index: Arc<dyn DirectoryIndex>,
        storage: Arc<dyn DirectoryStorage>,
        kind_registry: Arc<dyn DirectoryKindRegistry>,
    ) -> Self {
        Self {
            store,
            index,
            storage,
            kind_registry,
            mutation_lock: Arc::new(Mutex::new(())),
            action_ports: None,
        }
    }

    pub fn with_actions(
        mut self,
        registry: Arc<dyn DirectoryActionRegistry>,
        executor: Arc<dyn DirectoryActionExecutor>,
    ) -> Self {
        self.action_ports = Some(DirectoryActionPorts { registry, executor });
        self
    }

    pub fn describe_kind_actions(&self, kind: &DirectoryKind) -> Vec<DirectoryActionDefinition> {
        self.action_ports
            .as_ref()
            .map(|ports| {
                ports
                    .registry
                    .actions_for_kinds(&self.kind_registry.lineage(kind))
            })
            .unwrap_or_default()
    }

    pub fn kind_definitions(&self) -> &[crate::port::DirectoryKindDefinition] {
        self.kind_registry.definitions()
    }

    pub fn kind_lineage(&self, kind: &DirectoryKind) -> Vec<DirectoryKind> {
        self.kind_registry.lineage(kind)
    }

    pub fn describe_actions(&self, directory: &Directory) -> Result<DirectoryActions, CoreError> {
        self.ensure_kind_registered(directory.kind())?;
        Ok(DirectoryActions::new(
            self.describe_kind_actions(directory.kind())
                .into_iter()
                .filter(|action| action.matches_directory(directory.kind().as_str()))
                .collect(),
        ))
    }

    pub fn resolve_action(
        &self,
        directory: &Directory,
        action_id: &DirectoryAction,
    ) -> Result<DirectoryActionDefinition, CoreError> {
        self.describe_kind_actions(directory.kind())
            .into_iter()
            .find(|action| {
                action.id() == action_id && action.matches_directory(directory.kind().as_str())
            })
            .ok_or_else(|| {
                CoreError::configuration(format!(
                    "directory kind `{}` does not support action `{action_id}`",
                    directory.kind()
                ))
            })
    }

    pub async fn execute_action(
        &self,
        id: &DirectoryId,
        command: ExecuteDirectoryAction,
    ) -> Result<DirectoryActionOutput, CoreError> {
        let executed = self.invoke_action(id, command).await?;
        self.apply_executed_action(&executed, None).await?;
        Ok(executed.into_output())
    }

    pub(crate) async fn invoke_action(
        &self,
        id: &DirectoryId,
        command: ExecuteDirectoryAction,
    ) -> Result<ExecutedDirectoryAction, CoreError> {
        let located = self.find_by_id(id).await?;
        let expected_updated_at = located.directory().updated_at();
        let definition = self.resolve_action(located.directory(), &command.action)?;
        let ports = self.action_ports.as_ref().ok_or_else(|| {
            CoreError::configuration("directory action executor is not configured")
        })?;
        let output = ports
            .executor
            .execute(DirectoryActionRequest::new(
                located,
                command.action.clone(),
                definition.handler(),
                definition.access(),
                definition.requirements().clone(),
                command.input,
            ))
            .await?;
        self.validate_action_output(id, &command.action, &definition, &output)?;
        Ok(ExecutedDirectoryAction {
            directory_id: *id,
            expected_updated_at,
            access: definition.access(),
            output,
        })
    }

    pub(crate) async fn apply_executed_action(
        &self,
        executed: &ExecutedDirectoryAction,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<(), CoreError> {
        self.apply_action_effects(
            &executed.directory_id,
            executed.expected_updated_at,
            executed.access,
            &executed.output,
            required_parent_ancestor,
        )
        .await
    }

    fn validate_action_output(
        &self,
        directory_id: &DirectoryId,
        action_id: &DirectoryAction,
        definition: &DirectoryActionDefinition,
        output: &DirectoryActionOutput,
    ) -> Result<(), CoreError> {
        if output.directory_id() != *directory_id || output.action() != action_id {
            return Err(CoreError::configuration(format!(
                "action `{action_id}` returned an output for a different invocation"
            )));
        }
        let actual = output.output().view.kind();
        if !definition.output().view.iter().any(|view| view == actual) {
            return Err(CoreError::configuration(format!(
                "action `{}` returned undeclared view `{actual}`",
                definition.id()
            )));
        }
        if output.output().effects.len() > 1 {
            return Err(CoreError::configuration(format!(
                "action `{}` returned more than one directory effect",
                definition.id()
            )));
        }
        if output
            .output()
            .effects
            .iter()
            .filter(|effect| matches!(effect, DirectoryActionEffect::Update(_)))
            .count()
            > 1
        {
            return Err(CoreError::configuration(format!(
                "action `{}` returned more than one directory update effect",
                definition.id()
            )));
        }
        Ok(())
    }

    async fn apply_action_effects(
        &self,
        id: &DirectoryId,
        expected: DateTime<Utc>,
        access: DirectoryActionAccess,
        output: &DirectoryActionOutput,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<(), CoreError> {
        if output.output().effects.is_empty() {
            return Ok(());
        }
        if !matches!(access, DirectoryActionAccess::ReadWrite) {
            return Err(CoreError::configuration(format!(
                "action `{}` returned effects without write access",
                output.action()
            )));
        }
        for effect in output
            .output()
            .effects
            .iter()
            .filter_map(|effect| match effect {
                DirectoryActionEffect::CreateChild(effect) => Some(effect),
                DirectoryActionEffect::Update(_) => None,
            })
        {
            let kind = effect
                .kind
                .as_ref()
                .map(|kind| DirectoryKind::try_new(kind.clone()))
                .transpose()?
                .unwrap_or_default();
            self.create_with_kind_guarded(
                id,
                effect.name.clone(),
                kind,
                Some(expected),
                required_parent_ancestor,
            )
            .await?;
        }
        if let Some(effect) = output
            .output()
            .effects
            .iter()
            .find_map(|effect| match effect {
                DirectoryActionEffect::Update(effect) => Some(effect),
                DirectoryActionEffect::CreateChild(_) => None,
            })
        {
            let mut command = UpdateDirectory::new();
            if let Some(name) = &effect.name {
                command = command.with_name(name.clone());
            }
            if let Some(parent_id) = &effect.parent_id {
                command = command.with_parent_id(
                    DirectoryId::from_str(parent_id)
                        .map_err(|error| CoreError::configuration(error.to_string()))?,
                );
            }
            if let Some(kind) = &effect.kind {
                command = command.with_kind(DirectoryKind::try_new(kind.clone())?);
            }
            self.update_expected(id, command, Some(expected), required_parent_ancestor)
                .await?;
        }
        Ok(())
    }

    pub async fn reload_index(&self) -> Result<(), CoreError> {
        let directories = self.store.load_all().await?;
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

    /// Ensures every aggregate and physical directory in a path exists.
    pub async fn ensure_path(&self, path: &DirectoryPath) -> Result<DirectoryLocation, CoreError> {
        let _guard = self.mutation_lock.lock().await;
        if let Some(directory) = self.index.find_by_path(path).await? {
            self.storage.ensure_directory(path).await?;
            return Ok(directory.location().clone());
        }

        self.ensure_kind_registered(&DirectoryKind::default())?;
        self.storage.ensure_directory(path).await?;
        let mut parent = self
            .index
            .find_by_id(&DirectoryId::root())
            .await?
            .ok_or_else(|| CoreError::configuration("root directory is missing"))?;
        let mut current_path = DirectoryPath::root();
        for name in path.path().split('/').filter(|name| !name.is_empty()) {
            current_path = current_path.child(name)?;
            if let Some(existing) = self.index.find_by_path(&current_path).await? {
                parent = existing;
                continue;
            }
            let directory = Directory::new(parent.id(), name)?;
            self.store.insert(&directory).await?;
            self.update_index(directory.clone()).await?;
            parent = LocatedDirectory::new(
                directory.clone(),
                DirectoryLocation::new(directory.id(), current_path.clone()),
            )?;
        }
        Ok(parent.location().clone())
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

    pub async fn create(
        &self,
        parent: &DirectoryLocation,
        name: impl Into<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        self.create_with_kind(parent, name, DirectoryKind::default())
            .await
            .map(|directory| directory.location().clone())
    }

    pub async fn create_with_kind(
        &self,
        parent: &DirectoryLocation,
        name: impl Into<String>,
        kind: DirectoryKind,
    ) -> Result<LocatedDirectory, CoreError> {
        self.create_with_kind_guarded(&parent.id(), name, kind, None, None)
            .await
    }

    pub(crate) async fn create_with_kind_in_scope(
        &self,
        parent: &DirectoryLocation,
        name: impl Into<String>,
        kind: DirectoryKind,
        scope_root: DirectoryId,
    ) -> Result<LocatedDirectory, CoreError> {
        self.create_with_kind_guarded(&parent.id(), name, kind, None, Some(scope_root))
            .await
    }

    async fn create_with_kind_guarded(
        &self,
        parent_id: &DirectoryId,
        name: impl Into<String>,
        kind: DirectoryKind,
        expected_parent_updated_at: Option<DateTime<Utc>>,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<LocatedDirectory, CoreError> {
        let _guard = self.mutation_lock.lock().await;
        self.ensure_kind_registered(&kind)?;
        let parent = self
            .index
            .find_by_id(parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        if expected_parent_updated_at
            .is_some_and(|expected| expected != parent.directory().updated_at())
        {
            return Err(CoreError::conflict(format!(
                "directory `{parent_id}` changed while its action was executing"
            )));
        }
        if let Some(ancestor_id) = required_parent_ancestor
            && !self
                .index
                .is_descendant_or_self(&ancestor_id, parent_id)
                .await?
        {
            return Err(CoreError::forbidden("write", parent.path().path()));
        }
        let directory = Directory::new_with_kind(*parent_id, name, kind)?;
        let path = parent.path().child(directory.name())?;
        if self.index.find_by_path(&path).await?.is_some() {
            return Err(CoreError::conflict(
                "a directory with the same name already exists",
            ));
        }
        self.storage.ensure_directory(&path).await?;
        self.store.insert(&directory).await?;
        self.update_index(directory.clone()).await?;
        LocatedDirectory::new(
            directory.clone(),
            DirectoryLocation::new(directory.id(), path),
        )
    }

    pub async fn update(
        &self,
        id: &DirectoryId,
        command: UpdateDirectory,
    ) -> Result<LocatedDirectory, CoreError> {
        self.update_expected(id, command, None, None).await
    }

    async fn update_expected(
        &self,
        id: &DirectoryId,
        command: UpdateDirectory,
        action_expected: Option<DateTime<Utc>>,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<LocatedDirectory, CoreError> {
        let _guard = self.mutation_lock.lock().await;
        let located = self
            .index
            .find_by_id(id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", id.to_string()))?;
        let (mut directory, from) = located.into_parts();
        let expected_updated_at = directory.updated_at();
        if action_expected.is_some_and(|expected| expected != expected_updated_at) {
            return Err(CoreError::conflict(format!(
                "directory `{id}` changed while its action was executing"
            )));
        }

        if directory.id().is_root()
            && (command.name.is_some() || command.parent_id.is_some() || command.kind.is_some())
        {
            return Err(CoreError::conflict("root directory cannot be updated"));
        }
        if let Some(ancestor_id) = required_parent_ancestor
            && !self.index.is_descendant_or_self(&ancestor_id, id).await?
        {
            return Err(CoreError::forbidden("write", from.path().path()));
        }
        if let Some(kind) = command.kind {
            self.ensure_kind_registered(&kind)?;
            directory.change_kind(kind);
        }
        if let Some(name) = command.name {
            directory.rename(name)?;
        }
        let parent_id = command
            .parent_id
            .or(directory.parent_id())
            .ok_or_else(|| CoreError::configuration("non-root directory is missing its parent"))?;
        if self
            .index
            .is_descendant_or_self(&directory.id(), &parent_id)
            .await?
        {
            return Err(CoreError::conflict(
                "moving the directory would create a cycle",
            ));
        }
        directory.move_to(parent_id)?;
        let parent = self
            .index
            .find_by_id(&parent_id)
            .await?
            .ok_or_else(|| CoreError::not_found("directory", parent_id.to_string()))?;
        if let Some(ancestor_id) = required_parent_ancestor
            && !self
                .index
                .is_descendant_or_self(&ancestor_id, &parent_id)
                .await?
        {
            return Err(CoreError::forbidden("write", parent.path().path()));
        }
        let destination = parent.path().child(directory.name())?;
        if let Some(existing) = self.index.find_by_path(&destination).await?
            && existing.id() != directory.id()
        {
            return Err(CoreError::conflict(
                "a directory with the same name already exists",
            ));
        }
        if directory.updated_at() == expected_updated_at {
            return LocatedDirectory::new(directory, from);
        }

        let moved = destination != *from.path();
        if moved {
            self.storage
                .move_directory(from.path(), &destination)
                .await?;
        }
        let saved = self
            .store
            .save_if_unchanged(&directory, expected_updated_at)
            .await;
        let error = match saved {
            Ok(true) => {
                self.update_index(directory.clone()).await?;
                return LocatedDirectory::new(
                    directory.clone(),
                    DirectoryLocation::new(directory.id(), destination),
                );
            }
            Ok(false) => CoreError::conflict(format!(
                "directory `{}` changed while it was being updated",
                directory.id()
            )),
            Err(error) => error,
        };
        if moved && let Err(rollback) = self.storage.move_directory(&destination, from.path()).await
        {
            return Err(CoreError::storage("directory.update.rollback", rollback));
        }
        Err(error)
    }

    pub async fn rename(
        &self,
        id: &DirectoryId,
        name: impl Into<String>,
    ) -> Result<DirectoryLocation, CoreError> {
        self.update(id, UpdateDirectory::new().with_name(name))
            .await
            .map(|directory| directory.location().clone())
    }

    pub async fn move_to(
        &self,
        id: &DirectoryId,
        parent_id: &DirectoryId,
    ) -> Result<DirectoryLocation, CoreError> {
        self.update(id, UpdateDirectory::new().with_parent_id(*parent_id))
            .await
            .map(|directory| directory.location().clone())
    }

    pub async fn change_kind(
        &self,
        id: &DirectoryId,
        kind: DirectoryKind,
    ) -> Result<LocatedDirectory, CoreError> {
        self.update(id, UpdateDirectory::new().with_kind(kind))
            .await
    }

    pub async fn remove_if_empty(&self, directory: &DirectoryLocation) -> Result<bool, CoreError> {
        if directory.id().is_root() {
            return Ok(false);
        }
        let _guard = self.mutation_lock.lock().await;
        if self.store.remove_if_empty(&directory.id()).await? {
            self.index.remove(&directory.id()).await?;
            return Ok(true);
        }
        Ok(false)
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
            Err(CoreError::configuration(format!(
                "unsupported directory kind `{kind}`"
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
