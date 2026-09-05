use asset_core::CoreError;
use asset_core::domain::{
    DefinitionOrigin, DirectoryKind, DirectoryKindDefinition, ResourceKind, ResourceKindDefinition,
};

/// Host-owned static Kind definitions retained by the current backend.
pub struct BuiltinCatalog {
    pub(crate) resource_kinds: Vec<ResourceKindDefinition>,
    pub(crate) directory_kinds: Vec<DirectoryKindDefinition>,
}

impl BuiltinCatalog {
    pub fn new() -> Result<Self, CoreError> {
        Ok(Self {
            resource_kinds: vec![ResourceKindDefinition::new(
                ResourceKind::try_new(ResourceKind::DEFAULT)?,
                "Resource",
                true,
                DefinitionOrigin::builtin_static("core.resource"),
            )],
            directory_kinds: vec![DirectoryKindDefinition::new(
                DirectoryKind::default(),
                "Directory",
                DefinitionOrigin::builtin_static("core.directory"),
            )],
        })
    }
}
