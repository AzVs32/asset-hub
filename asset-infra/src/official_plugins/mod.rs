pub mod core_directory;
pub mod core_document;
pub mod core_image;
pub mod core_resource;
pub mod core_video;

pub const MANIFESTS: &[&str] = &[
    core_resource::MANIFEST,
    core_directory::MANIFEST,
    core_image::MANIFEST,
    core_document::MANIFEST,
    core_video::MANIFEST,
];
