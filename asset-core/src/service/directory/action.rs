use super::{
    DirectoryActionPorts, DirectoryActions, DirectoryService, ExecuteDirectoryAction,
    ExecutedDirectoryAction, UpdateDirectory,
};
use crate::service::validate_action_revision;
use crate::{
    CoreError,
    domain::{
        ActionAccess, Directory, DirectoryActionDefinition, DirectoryActionId, DirectoryId,
        DirectoryKind,
    },
    port::{
        DirectoryActionExecutor, DirectoryActionOutput, DirectoryActionRegistry,
        DirectoryActionRequest,
    },
};
use asset_plugin_api::protocol::directory::DirectoryActionEffect;
use std::str::FromStr;
use std::sync::Arc;

impl DirectoryService {
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

    pub fn describe_actions(&self, directory: &Directory) -> Result<DirectoryActions, CoreError> {
        Ok(DirectoryActions::new(
            self.available_actions_for_directory(directory)?,
        ))
    }

    pub fn resolve_action(
        &self,
        directory: &Directory,
        action_id: &DirectoryActionId,
    ) -> Result<DirectoryActionDefinition, CoreError> {
        self.available_actions_for_directory(directory)?
            .into_iter()
            .find(|action| action.id().as_str() == action_id.as_str())
            .ok_or_else(|| CoreError::unsupported("directory action", action_id.to_string()))
    }

    /// Resolve the authoritative action set for one Directory instance.
    ///
    /// `describe_kind_actions` has already selected definitions through the complete kind lineage
    /// and resolved nearest singleton providers. Rechecking a selected definition against only the
    /// concrete child kind would incorrectly discard inherited ancestor actions.
    fn available_actions_for_directory(
        &self,
        directory: &Directory,
    ) -> Result<Vec<DirectoryActionDefinition>, CoreError> {
        self.require_kind_registered(directory.kind())?;
        Ok(self
            .describe_kind_actions(directory.kind())
            .into_iter()
            .filter(|action| {
                !(directory.id().is_root()
                    && action
                        .output()
                        .effects
                        .iter()
                        .any(|effect| effect == "delete"))
            })
            .collect())
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
        let expected_revision = located.directory().revision();
        let definition = self.resolve_action(located.directory(), &command.action)?;
        validate_action_revision(
            definition.access(),
            command.expected_revision,
            expected_revision,
            "directory",
            id.to_string(),
        )?;
        let ports = self.action_ports.as_ref().ok_or_else(|| {
            CoreError::configuration("directory action executor is not configured")
        })?;
        let output = ports
            .executor
            .execute(DirectoryActionRequest::new(
                located,
                command.action.clone(),
                definition.access(),
                definition.requirements().clone(),
                command.input,
            ))
            .await?;
        self.validate_action_output(id, &command.action, &definition, &output)?;
        Ok(ExecutedDirectoryAction {
            directory_id: *id,
            expected_revision,
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
            executed.expected_revision,
            executed.access,
            &executed.output,
            required_parent_ancestor,
        )
        .await
    }

    fn validate_action_output(
        &self,
        directory_id: &DirectoryId,
        action_id: &DirectoryActionId,
        definition: &DirectoryActionDefinition,
        output: &DirectoryActionOutput,
    ) -> Result<(), CoreError> {
        if output.directory_id() != *directory_id || output.action() != action_id {
            return Err(CoreError::invariant(format!(
                "action `{action_id}` returned an output for a different invocation"
            )));
        }
        if let Some(view) = &output.output().view {
            let actual = view.kind();
            if !definition.output().views.iter().any(|view| view == actual) {
                return Err(CoreError::invariant(format!(
                    "action `{}` returned undeclared view `{actual}`",
                    definition.id()
                )));
            }
        }
        if output.output().view.is_none() && output.output().effects.is_empty() {
            return Err(CoreError::invariant(format!(
                "action `{}` returned neither a view nor an effect",
                definition.id()
            )));
        }
        if let Some(effect) = output.output().effects.iter().find(|effect| {
            !definition
                .output()
                .effects
                .iter()
                .any(|kind| kind == effect.kind())
        }) {
            return Err(CoreError::invariant(format!(
                "action `{}` returned undeclared effect `{}`",
                definition.id(),
                effect.kind()
            )));
        }
        if output.output().effects.len() > 1 {
            return Err(CoreError::invariant(format!(
                "action `{}` returned more than one directory effect",
                definition.id()
            )));
        }
        Ok(())
    }

    async fn apply_action_effects(
        &self,
        id: &DirectoryId,
        expected_revision: u64,
        access: ActionAccess,
        output: &DirectoryActionOutput,
        required_parent_ancestor: Option<DirectoryId>,
    ) -> Result<(), CoreError> {
        if output.output().effects.is_empty() {
            return Ok(());
        }
        if !matches!(access, ActionAccess::Write) {
            return Err(CoreError::invariant(format!(
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
                DirectoryActionEffect::Update(_) | DirectoryActionEffect::Delete => None,
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
                Some(expected_revision),
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
                DirectoryActionEffect::CreateChild(_) | DirectoryActionEffect::Delete => None,
            })
        {
            let mut command = UpdateDirectory::new(expected_revision);
            if let Some(name) = &effect.name {
                command = command.with_name(name.clone());
            }
            if let Some(parent_id) = &effect.parent_id {
                command = command.with_parent_id(
                    DirectoryId::from_str(parent_id)
                        .map_err(|error| CoreError::invariant(error.to_string()))?,
                );
            }
            if let Some(kind) = &effect.kind {
                command = command.with_kind(DirectoryKind::try_new(kind.clone())?);
            }
            self.update_expected(id, command, required_parent_ancestor)
                .await?;
        }
        if output
            .output()
            .effects
            .iter()
            .any(|effect| matches!(effect, DirectoryActionEffect::Delete))
        {
            let directory = self.find_by_id(id).await?;
            if !self
                .remove_if_empty(directory.location(), Some(expected_revision))
                .await?
            {
                return Err(CoreError::conflict(format!(
                    "directory `{id}` is not empty"
                )));
            }
        }
        Ok(())
    }
}
