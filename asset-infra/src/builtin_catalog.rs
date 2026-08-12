use asset_core::CoreError;
use asset_core::domain::{
    ActionAccess, ActionOutputContract, ActionUi as ActionDefinitionUi, DefinitionOrigin,
    DirectoryActionDefinition, DirectoryKind, DirectoryKindDefinition,
    ResourceActionContentDelivery, ResourceActionDefinition, ResourceActionRequirements,
    ResourceContentMatcher, ResourceKind, ResourceKindDefinition,
};

/// Host 内置的资源内容下载 action 稳定 ID。
const CORE_RESOURCE_DOWNLOAD: &str = "core.resource.download";
/// Host 内置的所有资源类型回退缩略图 provider 稳定 ID。
const CORE_RESOURCE_THUMBNAIL: &str = "core.resource.thumbnail";
/// Host 内置的 `core:image` 特化缩略图 provider 稳定 ID。
const CORE_IMAGE_THUMBNAIL: &str = "core.image.thumbnail";
/// Host 内置的 `core:text` 纯文本读取 action 稳定 ID。
const CORE_TEXT_READ: &str = "core.text.read";
/// Host 内置的 `core:text` 纯文本编辑 action 稳定 ID。
const CORE_TEXT_EDIT: &str = "core.text.edit";
/// 按类型层级解析最近缩略图 provider 的单例 capability。
pub(crate) const THUMBNAIL_CAPABILITY: &str = "thumbnail";
/// 按类型层级解析最近纯文本读取 provider 的单例 capability。
pub(crate) const TEXT_READ_CAPABILITY: &str = "text_read";
/// 按类型层级解析最近纯文本编辑 provider 的单例 capability。
pub(crate) const TEXT_EDIT_CAPABILITY: &str = "text_edit";
/// Host 内置的目录归档下载 action 稳定 ID。
const CORE_DIRECTORY_DOWNLOAD: &str = "core.directory.download";
/// Host 内置的所有目录类型回退缩略图 provider 稳定 ID。
const CORE_DIRECTORY_THUMBNAIL: &str = "core.directory.thumbnail";

/// 对外报告的 Host 内置根资源类型来源。
const CORE_RESOURCE_SOURCE: &str = "core.resource";
/// 对外报告的 Host 内置根目录类型来源。
const CORE_DIRECTORY_SOURCE: &str = "core.directory";

#[derive(Debug, Clone, Copy)]
pub(crate) enum BuiltinResourceHandler {
    Download,
    GenericThumbnail,
    ImageThumbnail,
    TextRead,
    TextEdit,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BuiltinDirectoryHandler {
    Download,
    GenericThumbnail,
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
        let image_kind = ResourceKind::try_new("core:image")?;
        let text_kind = ResourceKind::try_new("core:text")?;
        let video_kind = ResourceKind::try_new("core:video")?;
        let directory_kind = DirectoryKind::default();

        let resource_kinds = vec![
            ResourceKindDefinition::new(
                resource_kind.clone(),
                "Resource",
                true,
                DefinitionOrigin::builtin_static(CORE_RESOURCE_SOURCE),
            ),
            ResourceKindDefinition::new(
                image_kind,
                "Image",
                true,
                DefinitionOrigin::builtin_static("core.image"),
            )
            .with_parent(Some(resource_kind.clone()))
            .with_detect(
                ResourceContentMatcher::new()
                    .with_mime_types(["image/*"])
                    .with_extensions([
                        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif",
                    ]),
            ),
            ResourceKindDefinition::new(
                text_kind,
                "Text",
                true,
                DefinitionOrigin::builtin_static("core.text"),
            )
            .with_parent(Some(resource_kind.clone()))
            .with_detect(
                ResourceContentMatcher::new()
                    .with_mime_types([
                        "text/*",
                        "application/json",
                        "application/xml",
                        "application/toml",
                        "application/yaml",
                        "application/x-yaml",
                    ])
                    .with_extensions([
                        ".txt", ".text", ".log", ".csv", ".tsv", ".json", ".xml", ".toml", ".yaml",
                        ".yml",
                    ]),
            ),
            ResourceKindDefinition::new(
                video_kind,
                "Video",
                true,
                DefinitionOrigin::builtin_static("core.video"),
            )
            .with_parent(Some(resource_kind))
            .with_detect(
                ResourceContentMatcher::new()
                    .with_mime_types(["video/*"])
                    .with_extensions([".mp4", ".webm", ".mov", ".m4v", ".ogv"]),
            ),
        ];

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
                })
                .with_ui(ActionDefinitionUi {
                    group: Some("open".to_string()),
                    order: Some(10),
                    locations: vec!["resource_detail".to_string(), "context_menu".to_string()],
                }),
                handler: BuiltinResourceHandler::Download,
            },
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new_static(
                    CORE_RESOURCE_THUMBNAIL,
                    "Thumbnail",
                )
                .with_static_provides(Some(THUMBNAIL_CAPABILITY))
                .with_kinds([ResourceKind::DEFAULT])
                .with_output(ActionOutputContract {
                    views: vec!["media".to_string()],
                })
                .with_ui(ActionDefinitionUi {
                    group: Some("preview".to_string()),
                    order: Some(100),
                    locations: vec!["resource_thumbnail".to_string()],
                }),
                handler: BuiltinResourceHandler::GenericThumbnail,
            },
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new_static(
                    CORE_IMAGE_THUMBNAIL,
                    "Image Thumbnail",
                )
                .with_static_provides(Some(THUMBNAIL_CAPABILITY))
                .with_kinds(["core:image"])
                .with_output(ActionOutputContract {
                    views: vec!["media".to_string()],
                })
                .with_ui(ActionDefinitionUi {
                    group: Some("preview".to_string()),
                    order: Some(100),
                    locations: vec!["resource_thumbnail".to_string()],
                }),
                handler: BuiltinResourceHandler::ImageThumbnail,
            },
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new_static(CORE_TEXT_READ, "Read")
                    .with_static_provides(Some(TEXT_READ_CAPABILITY))
                    .with_kinds(["core:text"])
                    .with_requirements(ResourceActionRequirements {
                        content: true,
                        content_delivery: ResourceActionContentDelivery::Inline,
                    })
                    .with_output(ActionOutputContract {
                        views: vec!["text".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("open".to_string()),
                        order: Some(50),
                        locations: vec!["resource_detail".to_string(), "context_menu".to_string()],
                    }),
                handler: BuiltinResourceHandler::TextRead,
            },
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new_static(CORE_TEXT_EDIT, "Edit")
                    .with_static_provides(Some(TEXT_EDIT_CAPABILITY))
                    .with_access(ActionAccess::Write)
                    .with_kinds(["core:text"])
                    .with_requirements(ResourceActionRequirements {
                        content: true,
                        content_delivery: ResourceActionContentDelivery::Inline,
                    })
                    .with_output(ActionOutputContract {
                        views: vec!["text".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("edit".to_string()),
                        order: Some(50),
                        locations: vec!["resource_detail".to_string(), "context_menu".to_string()],
                    }),
                handler: BuiltinResourceHandler::TextEdit,
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
                })
                .with_ui(ActionDefinitionUi {
                    group: Some("open".to_string()),
                    order: Some(10),
                    locations: vec![
                        "directory_context_menu".to_string(),
                        "directory_detail".to_string(),
                    ],
                }),
                handler: BuiltinDirectoryHandler::Download,
            },
            BuiltinDirectoryAction {
                definition: DirectoryActionDefinition::new_static(
                    CORE_DIRECTORY_THUMBNAIL,
                    "Thumbnail",
                )
                .with_static_provides(Some(THUMBNAIL_CAPABILITY))
                .with_kinds([DirectoryKind::DEFAULT])
                .with_output(ActionOutputContract {
                    views: vec!["media".to_string()],
                })
                .with_ui(ActionDefinitionUi {
                    group: Some("preview".to_string()),
                    order: Some(100),
                    locations: vec!["directory_thumbnail".to_string()],
                }),
                handler: BuiltinDirectoryHandler::GenericThumbnail,
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
