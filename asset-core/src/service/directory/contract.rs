use crate::domain::{
    ActionAccess, DirectoryActionDefinition, DirectoryActionId, DirectoryId, DirectoryKind,
    IdempotencyKey,
};
use crate::port::DirectoryActionOutput;
use serde_json::Value;

/// A partial update to a directory aggregate.
#[derive(Debug, Clone, Default)]
pub struct UpdateDirectory {
    pub(super) expected_revision: u64,
    pub(super) name: Option<String>,
    pub(super) parent_id: Option<DirectoryId>,
    pub(super) kind: Option<DirectoryKind>,
}

impl UpdateDirectory {
    pub fn new(expected_revision: u64) -> Self {
        Self {
            expected_revision,
            ..Self::default()
        }
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
    pub action: DirectoryActionId,
    pub input: Value,
    pub expected_revision: Option<u64>,
    pub(super) idempotency_key: Option<IdempotencyKey>,
}

impl ExecuteDirectoryAction {
    pub fn new(action: DirectoryActionId, expected_revision: Option<u64>) -> Self {
        Self {
            action,
            input: Value::Object(Default::default()),
            expected_revision,
            idempotency_key: None,
        }
    }

    pub fn with_input(mut self, input: Value) -> Self {
        self.input = input;
        self
    }

    pub fn with_idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    pub fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DirectoryActions {
    available_actions: Vec<DirectoryActionDefinition>,
}

impl DirectoryActions {
    pub(crate) fn new(available_actions: Vec<DirectoryActionDefinition>) -> Self {
        Self { available_actions }
    }

    pub fn available_actions(&self) -> &[DirectoryActionDefinition] {
        &self.available_actions
    }
}

pub(crate) struct ExecutedDirectoryAction {
    directory_id: DirectoryId,
    expected_revision: u64,
    access: ActionAccess,
    output: DirectoryActionOutput,
}

impl ExecutedDirectoryAction {
    pub(crate) fn new(
        directory_id: DirectoryId,
        expected_revision: u64,
        access: ActionAccess,
        output: DirectoryActionOutput,
    ) -> Self {
        Self {
            directory_id,
            expected_revision,
            access,
            output,
        }
    }

    pub(crate) fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }

    pub(crate) fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub(crate) fn access(&self) -> ActionAccess {
        self.access
    }

    pub(crate) fn output(&self) -> &DirectoryActionOutput {
        &self.output
    }

    pub(crate) fn into_output(self) -> DirectoryActionOutput {
        self.output
    }
}
