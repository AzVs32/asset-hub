use crate::{
    CoreError,
    domain::{DirectoryId, DirectoryKind},
    port::LocatedDirectory,
};
use asset_plugin_api::{
    DirectoryAction, DirectoryActionAccess, DirectoryActionDefinition, DirectoryPluginActionOutput,
};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct DirectoryActionRequest {
    directory: LocatedDirectory,
    action: DirectoryAction,
    handler: Option<String>,
    access: DirectoryActionAccess,
    requirements: asset_plugin_api::DirectoryActionRequirements,
    input: Value,
}

impl DirectoryActionRequest {
    pub fn new(
        directory: LocatedDirectory,
        action: DirectoryAction,
        handler: Option<impl Into<String>>,
        access: DirectoryActionAccess,
        requirements: asset_plugin_api::DirectoryActionRequirements,
        input: Value,
    ) -> Self {
        Self {
            directory,
            action,
            handler: handler.map(Into::into),
            access,
            requirements,
            input,
        }
    }
    pub fn directory(&self) -> &LocatedDirectory {
        &self.directory
    }
    pub fn action(&self) -> &DirectoryAction {
        &self.action
    }
    pub fn handler(&self) -> Option<&str> {
        self.handler.as_deref()
    }
    pub fn access(&self) -> DirectoryActionAccess {
        self.access
    }
    pub fn requirements(&self) -> &asset_plugin_api::DirectoryActionRequirements {
        &self.requirements
    }
    pub fn input(&self) -> &Value {
        &self.input
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirectoryActionOutput {
    directory_id: DirectoryId,
    action: DirectoryAction,
    output: DirectoryPluginActionOutput,
}

impl DirectoryActionOutput {
    pub fn new(
        directory_id: DirectoryId,
        action: DirectoryAction,
        output: DirectoryPluginActionOutput,
    ) -> Self {
        Self {
            directory_id,
            action,
            output,
        }
    }
    pub fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }
    pub fn action(&self) -> &DirectoryAction {
        &self.action
    }
    pub fn output(&self) -> &DirectoryPluginActionOutput {
        &self.output
    }
}

#[async_trait]
pub trait DirectoryActionExecutor: Send + Sync {
    async fn execute(
        &self,
        request: DirectoryActionRequest,
    ) -> Result<DirectoryActionOutput, CoreError>;
}

pub trait DirectoryActionRegistry: Send + Sync {
    fn actions(&self) -> &[DirectoryActionDefinition];

    fn actions_for_kinds(&self, kinds: &[DirectoryKind]) -> Vec<DirectoryActionDefinition> {
        let mut selected = Vec::new();
        for kind in kinds {
            for action in self.actions().iter().filter(|action| {
                action
                    .kinds()
                    .iter()
                    .any(|expected| expected.eq_ignore_ascii_case(kind.as_str()))
            }) {
                if !selected
                    .iter()
                    .any(|existing: &DirectoryActionDefinition| existing.id() == action.id())
                {
                    selected.push(action.clone());
                }
            }
        }
        for action in self
            .actions()
            .iter()
            .filter(|action| action.kinds().is_empty())
        {
            if !selected
                .iter()
                .any(|existing: &DirectoryActionDefinition| existing.id() == action.id())
            {
                selected.push(action.clone());
            }
        }
        selected
    }
}
