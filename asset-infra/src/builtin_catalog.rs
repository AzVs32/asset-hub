use asset_core::CoreError;
use asset_core::domain::{
    ActionAccess, ActionOutputContract, ActionUi as ActionDefinitionUi, DefinitionOrigin,
    DirectoryActionDefinition, DirectoryKind, DirectoryKindDefinition,
    ResourceActionContentDelivery, ResourceActionDefinition, ResourceActionRequirements,
    ResourceKind, ResourceKindDefinition,
};

/// Host 内置的资源内容下载 action 稳定 ID。
const CORE_RESOURCE_DOWNLOAD: &str = "core.resource.download";
/// Host 内置的资源软删除命令稳定 ID。
const CORE_RESOURCE_DELETE: &str = "core.resource.delete";
/// Host 内置的目录归档下载 action 稳定 ID。
const CORE_DIRECTORY_DOWNLOAD: &str = "core.directory.download";
/// Host 内置的空目录删除命令稳定 ID。
const CORE_DIRECTORY_DELETE: &str = "core.directory.delete";

/// 对外报告的 Host 内置根资源类型来源。
const CORE_RESOURCE_SOURCE: &str = "core.resource";
/// 对外报告的 Host 内置根目录类型来源。
const CORE_DIRECTORY_SOURCE: &str = "core.directory";

#[derive(Debug, Clone, Copy)]
pub(crate) enum BuiltinResourceHandler {
    Delete,
    Download,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BuiltinDirectoryHandler {
    Delete,
    Download,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltinResourceAction {
    pub(crate) definition: ResourceActionDefinition,
    pub(crate) handler: BuiltinResourceHandler,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltinDirectoryAction {
    pub(crate) definition: DirectoryActionDefinition,
    pub(crate) handler: BuiltinDirectoryHandler,
}

#[derive(Debug, Clone)]
pub(crate) struct BuiltinCatalog {
    pub(crate) resource_kinds: Vec<ResourceKindDefinition>,
    pub(crate) directory_kinds: Vec<DirectoryKindDefinition>,
    pub(crate) resource_actions: Vec<BuiltinResourceAction>,
    pub(crate) directory_actions: Vec<BuiltinDirectoryAction>,
}

impl BuiltinCatalog {
    pub(crate) fn new() -> Result<Self, CoreError> {
        let resource_kind = ResourceKind::try_new(ResourceKind::DEFAULT)?;
        let directory_kind = DirectoryKind::default();

        let resource_kinds = vec![ResourceKindDefinition::new(
            resource_kind,
            "Resource",
            true,
            DefinitionOrigin::builtin_static(CORE_RESOURCE_SOURCE),
        )];

        let directory_kinds = vec![DirectoryKindDefinition::new(
            directory_kind,
            "Directory",
            DefinitionOrigin::builtin_static(CORE_DIRECTORY_SOURCE),
        )];

        let resource_actions = vec![
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new_static(
                    CORE_RESOURCE_DOWNLOAD,
                    "Download",
                )
                .with_kinds([ResourceKind::DEFAULT])
                .with_requirements(ResourceActionRequirements {
                    content: true,
                    content_delivery: ResourceActionContentDelivery::Reference,
                })
                .with_output(ActionOutputContract {
                    views: vec!["download".to_string()],
                    effects: Vec::new(),
                })
                .with_ui(ActionDefinitionUi {
                    group: Some("open".to_string()),
                    order: Some(10),
                    locations: vec!["resource_context_menu".to_string()],
                    ..ActionDefinitionUi::default()
                }),
                handler: BuiltinResourceHandler::Download,
            },
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new_static(CORE_RESOURCE_DELETE, "Delete")
                    .with_access(ActionAccess::Write)
                    .with_kinds([ResourceKind::DEFAULT])
                    .with_output(ActionOutputContract {
                        views: Vec::new(),
                        effects: vec!["delete".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("danger".to_string()),
                        order: Some(100),
                        locations: vec!["resource_context_menu".to_string()],
                        destructive: true,
                        confirmation: Some("Delete {name}?".to_string()),
                    }),
                handler: BuiltinResourceHandler::Delete,
            },
        ];

        let directory_actions = vec![
            BuiltinDirectoryAction {
                definition: DirectoryActionDefinition::new_static(
                    CORE_DIRECTORY_DOWNLOAD,
                    "Download",
                )
                .with_kinds([DirectoryKind::DEFAULT])
                .with_output(ActionOutputContract {
                    views: vec!["download".to_string()],
                    effects: Vec::new(),
                })
                .with_ui(ActionDefinitionUi {
                    group: Some("open".to_string()),
                    order: Some(10),
                    locations: vec!["directory_context_menu".to_string()],
                    ..ActionDefinitionUi::default()
                }),
                handler: BuiltinDirectoryHandler::Download,
            },
            BuiltinDirectoryAction {
                definition: DirectoryActionDefinition::new_static(CORE_DIRECTORY_DELETE, "Delete")
                    .with_access(ActionAccess::Write)
                    .with_kinds([DirectoryKind::DEFAULT])
                    .with_output(ActionOutputContract {
                        views: Vec::new(),
                        effects: vec!["delete".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("danger".to_string()),
                        order: Some(100),
                        locations: vec!["directory_context_menu".to_string()],
                        destructive: true,
                        confirmation: Some("Delete empty directory {name}?".to_string()),
                    }),
                handler: BuiltinDirectoryHandler::Delete,
            },
        ];

        Ok(Self {
            resource_kinds,
            directory_kinds,
            resource_actions,
            directory_actions,
        })
    }
}
