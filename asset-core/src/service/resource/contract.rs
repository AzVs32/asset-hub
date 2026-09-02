//! Public application contracts for the independently assembled Resource-related services.

use crate::domain::{Checksum, DirectoryId, ResourceActionDefinition, ResourceActionId, ResourceKind};
use crate::port::BlobByteStream;

#[derive(Debug, Clone)]
pub struct CreateUpload {
    pub(super) name: String,
    pub(super) kind: Option<ResourceKind>,
    pub(super) directory_id: DirectoryId,
    pub(super) mime_type: Option<String>,
    pub(super) expected_size: u64,
    pub(super) expected_checksum: Checksum,
}

impl CreateUpload {
    pub fn new(
        name: impl Into<String>,
        directory_id: DirectoryId,
        expected_size: u64,
        expected_checksum: Checksum,
    ) -> Self {
        Self {
            name: name.into(),
            kind: None,
            directory_id,
            mime_type: None,
            expected_size,
            expected_checksum,
        }
    }

    pub fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn directory_id(&self) -> DirectoryId {
        self.directory_id
    }
}

#[derive(Debug, Clone)]
pub struct ExecuteResourceAction {
    pub(super) action: ResourceActionId,
    pub(super) input: serde_json::Value,
    pub(super) expected_revision: Option<u64>,
}

impl ExecuteResourceAction {
    pub fn new(action: ResourceActionId, expected_revision: Option<u64>) -> Self {
        Self {
            action,
            input: serde_json::Value::Object(Default::default()),
            expected_revision,
        }
    }

    pub fn with_input(mut self, input: serde_json::Value) -> Self {
        self.input = input;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ReplaceResourceContent {
    pub(super) expected_size: u64,
    pub(super) expected_checksum: Checksum,
    pub(super) expected_revision: u64,
    pub(super) mime_type: Option<String>,
}

impl ReplaceResourceContent {
    pub fn new(expected_size: u64, expected_checksum: Checksum, expected_revision: u64) -> Self {
        Self {
            expected_size,
            expected_checksum,
            expected_revision,
            mime_type: None,
        }
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct UpdateResource {
    pub(super) expected_revision: u64,
    pub(super) name: Option<String>,
    pub(super) directory_id: Option<DirectoryId>,
    pub(super) kind: Option<ResourceKind>,
}

impl UpdateResource {
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

    pub fn with_directory_id(mut self, directory_id: DirectoryId) -> Self {
        self.directory_id = Some(directory_id);
        self
    }

    pub fn with_kind(mut self, kind: ResourceKind) -> Self {
        self.kind = Some(kind);
        self
    }

    pub fn directory_id(&self) -> Option<DirectoryId> {
        self.directory_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceActions {
    available_actions: Vec<ResourceActionDefinition>,
}

impl ResourceActions {
    pub(super) fn new(available_actions: Vec<ResourceActionDefinition>) -> Self {
        Self { available_actions }
    }

    pub fn available_actions(&self) -> &[ResourceActionDefinition] {
        &self.available_actions
    }
}

pub struct ResourceContentStream {
    content_type: String,
    content_length: u64,
    content: BlobByteStream,
}

impl ResourceContentStream {
    pub(super) fn new(content_type: String, content_length: u64, content: BlobByteStream) -> Self {
        Self {
            content_type,
            content_length,
            content,
        }
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn content_length(&self) -> u64 {
        self.content_length
    }

    pub fn into_content(self) -> BlobByteStream {
        self.content
    }
}
