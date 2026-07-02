pub mod core_document;
pub mod core_file;
pub mod core_image;
pub mod core_video;

pub const MANIFESTS: &[&str] = &[
    core_file::MANIFEST,
    core_image::MANIFEST,
    core_document::MANIFEST,
    core_video::MANIFEST,
];
