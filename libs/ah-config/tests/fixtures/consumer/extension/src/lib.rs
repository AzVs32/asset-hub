//! A declaration-only dependency with private types and renamed runtime dependency.

#[derive(settings::Config)]
#[config(key = "extension", spaces = ["plugins", "extension_only"])]
struct ExtensionConfig {
    port: u16,
}

/// A namespace exported for configurations declared in dependent crates.
#[derive(settings::Config)]
#[config(key = "vfs", default)]
pub struct VfsConfig {
    #[config(default = true)]
    pub enabled: bool,
}

#[derive(settings::Config)]
#[config(key = "closed_vfs", spaces = ["closed"], deny_unknown_fields)]
pub struct ClosedVfsConfig {}
