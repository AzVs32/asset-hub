use super::*;
use crate::builtin_catalog::BuiltinCatalog;
use asset_core::CoreError;

/// Resource and Directory Kind registries built from the Host-owned static catalog.
pub struct KindCatalogs {
    pub resource_kinds: DefaultResourceKindRegistry,
    pub directory_kinds: DefaultDirectoryKindRegistry,
}

pub fn build_kind_catalogs(catalog: &BuiltinCatalog) -> Result<KindCatalogs, CoreError> {
    Ok(KindCatalogs {
        resource_kinds: DefaultResourceKindRegistry::from_definitions(
            catalog.resource_kinds.clone(),
        ),
        directory_kinds: directory_registry_from_builtin(catalog)?,
    })
}
