//! A declaration-only dependency with private types and renamed runtime dependency.

#[derive(settings::Config)]
#[config(key = "extension", spaces = ["plugins", "extension_only"])]
struct ExtensionConfig {
    port: u16,
}
