use crate::domain::{
    DirectoryAction, DirectoryActionAccess, DirectoryActionDefinition, DirectoryId, DirectoryKind,
};
use crate::port::DirectoryActionOutput;
use serde_json::Value;

/// A partial update to a directory aggregate.
#[derive(Debug, Clone, Default)]
pub struct UpdateDirectory {
    pub(super) name: Option<String>,
    pub(super) parent_id: Option<DirectoryId>,
    pub(super) kind: Option<DirectoryKind>,
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
    pub fn new(action: DirectoryAction) -> Self {
        Self {
            action,
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

impl DirectoryActions {
    pub(super) fn new(available_actions: Vec<DirectoryActionDefinition>) -> Self {
        Self { available_actions }
    }

    pub fn available_actions(&self) -> &[DirectoryActionDefinition] {
        &self.available_actions
    }
}

pub(crate) struct ExecutedDirectoryAction {
    pub(super) directory_id: DirectoryId,
    pub(super) expected_revision: u64,
    pub(super) access: DirectoryActionAccess,
    pub(super) output: DirectoryActionOutput,
}

impl ExecutedDirectoryAction {
    pub(crate) fn into_output(self) -> DirectoryActionOutput {
        self.output
    }
}
