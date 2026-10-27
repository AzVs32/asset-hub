//! Select host and plugin configuration independently from one TOML file.
//!
//! Explicit spaces replace implicit default membership. A logging declaration
//! belongs to both spaces, while host and plugin settings remain isolated.
//! Space names select declarations; they do not prefix the TOML section names.
//! A shared declaration belongs to both loading plans, while a private type
//! cannot be retrieved from the other space even if its section is in the file.
//! Validators run only for selected declarations: an invalid plugin directory
//! does not prevent loading the host space, but rejects the plugins space.
//!
//! The sibling TOML file is located through the source directory for this example.
//! Run from the ah-config source root: `cargo run --example spaces`.

use ah_config::{ConfigErrorKind, Registry};
use std::path::PathBuf;

#[derive(ah_config::Config)]
#[config(key = "http", validate = validate_http)]
struct HttpConfig {
    port: u16,
}

#[derive(ah_config::Config)]
#[config(key = "plugin_runtime", spaces = ["plugins"], validate = validate_plugin_runtime)]
struct PluginRuntimeConfig {
    directory: PathBuf,
}

#[derive(ah_config::Config)]
#[config(key = "logging", spaces = ["default", "plugins"])]
struct LoggingConfig {
    level: String,
}

fn validate_http(config: &HttpConfig) -> Result<(), &'static str> {
    if config.port == 0 {
        Err("HTTP port must be nonzero")
    } else {
        Ok(())
    }
}

fn validate_plugin_runtime(config: &PluginRuntimeConfig) -> Result<(), &'static str> {
    if config.directory.as_os_str().is_empty() {
        Err("plugin directory cannot be empty")
    } else {
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Available spaces: {:?}", Registry::spaces());
    for section in Registry::declarations() {
        println!(
            "{}: {} in {:?}",
            section.key(),
            section.type_name(),
            section.spaces()
        );
    }
    let registry = Registry::for_space("default")?;
    assert_eq!(registry.sections().len(), 2);
    let host = registry.load_required("examples/spaces.toml")?;
    let plugins = Registry::for_space("plugins")?.load_required("examples/spaces.toml")?;
    assert_eq!(host.space(), "default");
    assert_eq!(plugins.space(), "plugins");
    assert_eq!(host.get::<HttpConfig>()?.port, 8080);
    assert_eq!(
        plugins.get::<PluginRuntimeConfig>()?.directory,
        PathBuf::from("plugins")
    );
    assert_eq!(host.get::<LoggingConfig>()?.level, "info");
    assert_eq!(plugins.get::<LoggingConfig>()?.level, "info");

    // A section being present in the file does not make it available in every space.
    assert_eq!(
        plugins.get::<HttpConfig>().err().map(|error| error.kind()),
        Some(ConfigErrorKind::NotRegistered)
    );
    assert_eq!(
        host.get::<PluginRuntimeConfig>()
            .err()
            .map(|error| error.kind()),
        Some(ConfigErrorKind::NotRegistered)
    );
    println!("default space: http + logging; plugins space: plugin_runtime + logging.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn isolates_space_membership() -> Result<(), Box<dyn std::error::Error>> {
        super::main()
    }

    #[test]
    fn validates_only_the_selected_space() -> Result<(), Box<dyn std::error::Error>> {
        let path = "examples/spaces_invalid_plugin.toml";
        let host = super::Registry::for_space("default")?.load_required(path)?;
        assert_eq!(host.get::<super::HttpConfig>()?.port, 8080);
        let error = super::Registry::for_space("plugins")?
            .load_required(path)
            .err()
            .ok_or("expected plugin directory validation to fail")?;
        assert_eq!(error.kind(), super::ConfigErrorKind::Validation);
        Ok(())
    }
}
