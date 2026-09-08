use crate::directory::domain::DirectoryId;

/// A partial update to a directory aggregate.
#[derive(Debug, Clone, Default)]
pub struct UpdateDirectory {
    pub(super) expected_revision: u64,
    pub(super) name: Option<String>,
    pub(super) parent_id: Option<DirectoryId>,
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
}
