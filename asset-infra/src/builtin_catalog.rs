use asset_core::CoreError;
use asset_core::domain::{
    ActionOutputContract, ActionUi as ActionDefinitionUi, DirectoryActionDefinition, DirectoryKind,
    ResourceActionContentDelivery, ResourceActionDefinition, ResourceActionRequirements,
    ResourceContentMatcher, ResourceKind,
};
use asset_core::port::{DirectoryKindDefinition, ResourceKindDefinition};

const CORE_RESOURCE_DOWNLOAD: &str = "core.resource.download";
pub(crate) const CORE_RESOURCE_THUMBNAIL: &str = "core.resource.thumbnail";
pub(crate) const CORE_IMAGE_THUMBNAIL: &str = "core.image.thumbnail";
pub(crate) const THUMBNAIL_CAPABILITY: &str = "thumbnail";
const CORE_DIRECTORY_DOWNLOAD: &str = "core.directory.download";
pub(crate) const CORE_DIRECTORY_THUMBNAIL: &str = "core.directory.thumbnail";

const CORE_RESOURCE_SOURCE: &str = "builtin:core.resource";
const CORE_DIRECTORY_SOURCE: &str = "builtin:core.directory";

#[derive(Debug, Clone, Copy)]
pub(crate) enum BuiltinResourceHandler {
    Download,
    ResourceThumbnail,
    ImageThumbnail,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum BuiltinDirectoryHandler {
    Download,
    Thumbnail,
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
        let document_kind = ResourceKind::try_new("core:document")?;
        let video_kind = ResourceKind::try_new("core:video")?;
        let directory_kind = DirectoryKind::default();

        let resource_kinds = vec![
            ResourceKindDefinition::with_source(
                resource_kind.clone(),
                "Resource",
                true,
                CORE_RESOURCE_SOURCE,
            ),
            ResourceKindDefinition::with_source(image_kind, "Image", true, "builtin:core.image")
                .with_parent(Some(resource_kind.clone()))
                .with_detect(
                    ResourceContentMatcher::new()
                        .with_mime_types(["image/*"])
                        .with_extensions([
                            ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".avif",
                        ]),
                ),
            ResourceKindDefinition::with_source(
                document_kind,
                "Document",
                true,
                "builtin:core.document",
            )
            .with_parent(Some(resource_kind.clone()))
            .with_detect(
                ResourceContentMatcher::new()
                    .with_mime_types(["application/pdf"])
                    .with_extensions([".pdf"]),
            ),
            ResourceKindDefinition::with_source(video_kind, "Video", true, "builtin:core.video")
                .with_parent(Some(resource_kind))
                .with_detect(
                    ResourceContentMatcher::new()
                        .with_mime_types(["video/*"])
                        .with_extensions([".mp4", ".webm", ".mov", ".m4v", ".ogv"]),
                ),
        ];

        let directory_kinds = vec![DirectoryKindDefinition::with_source(
            directory_kind,
            "Directory",
            CORE_DIRECTORY_SOURCE,
        )];

        let resource_actions = vec![
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new(CORE_RESOURCE_DOWNLOAD, "Download")
                    .with_kinds([ResourceKind::DEFAULT])
                    .with_requirements(ResourceActionRequirements {
                        content: true,
                        content_delivery: ResourceActionContentDelivery::Reference,
                    })
                    .with_output(ActionOutputContract {
                        view: vec!["download".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("open".to_string()),
                        order: Some(10),
                        locations: vec!["resource_detail".to_string(), "context_menu".to_string()],
                    }),
                handler: BuiltinResourceHandler::Download,
            },
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new(CORE_RESOURCE_THUMBNAIL, "Thumbnail")
                    .with_provides(Some(THUMBNAIL_CAPABILITY))
                    .with_kinds([ResourceKind::DEFAULT])
                    .with_output(ActionOutputContract {
                        view: vec!["media".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("preview".to_string()),
                        order: Some(100),
                        locations: vec!["resource_list_thumbnail".to_string()],
                    }),
                handler: BuiltinResourceHandler::ResourceThumbnail,
            },
            BuiltinResourceAction {
                definition: ResourceActionDefinition::new(CORE_IMAGE_THUMBNAIL, "Image Thumbnail")
                    .with_provides(Some(THUMBNAIL_CAPABILITY))
                    .with_kinds(["core:image"])
                    .with_output(ActionOutputContract {
                        view: vec!["media".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("preview".to_string()),
                        order: Some(100),
                        locations: vec!["resource_list_thumbnail".to_string()],
                    }),
                handler: BuiltinResourceHandler::ImageThumbnail,
            },
        ];

        let directory_actions = vec![
            BuiltinDirectoryAction {
                definition: DirectoryActionDefinition::new(CORE_DIRECTORY_DOWNLOAD, "Download")
                    .with_kinds([DirectoryKind::DEFAULT])
                    .with_output(ActionOutputContract {
                        view: vec!["download".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("open".to_string()),
                        order: Some(10),
                        locations: vec![
                            "directory_toolbar".to_string(),
                            "directory_context_menu".to_string(),
                            "directory_detail".to_string(),
                        ],
                    }),
                handler: BuiltinDirectoryHandler::Download,
            },
            BuiltinDirectoryAction {
                definition: DirectoryActionDefinition::new(CORE_DIRECTORY_THUMBNAIL, "Thumbnail")
                    .with_provides(Some(THUMBNAIL_CAPABILITY))
                    .with_kinds([DirectoryKind::DEFAULT])
                    .with_output(ActionOutputContract {
                        view: vec!["media".to_string()],
                    })
                    .with_ui(ActionDefinitionUi {
                        group: Some("preview".to_string()),
                        order: Some(100),
                        locations: vec!["directory_list_thumbnail".to_string()],
                    }),
                handler: BuiltinDirectoryHandler::Thumbnail,
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
