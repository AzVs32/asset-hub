use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CoreConfig {
    workers: usize,
}

impl ConfigSection for CoreConfig {
    const SECTION: &'static str = "asset";

    fn normalize(mut self) -> Result<Self, String> {
        if self.workers == 0 {
            self.workers = 4;
        }
        Ok(self)
    }
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct PluginConfig {
    enabled: bool,
}

impl ConfigSection for PluginConfig {
    const SECTION: &'static str = "plugins.preview";
}

#[test]
fn independently_registered_sections_share_one_document() {
    let mut registry = ConfigRegistry::new();
    registry
        .register::<CoreConfig>()
        .unwrap()
        .register::<PluginConfig>()
        .unwrap();

    let loaded = registry
        .load_str("[asset]\nworkers = 8\n[plugins.preview]\nenabled = true")
        .unwrap();

    assert_eq!(loaded.section::<CoreConfig>().unwrap().workers, 8);
    assert!(loaded.section::<PluginConfig>().unwrap().enabled);
}

#[test]
fn unregistered_extensions_survive_normalized_output() {
    let mut registry = ConfigRegistry::new();
    registry.register::<CoreConfig>().unwrap();
    let loaded = registry
        .load_str("[plugins.unknown]\nsetting = \"preserved\"")
        .unwrap();

    let output = loaded.to_toml_string().unwrap();
    assert!(output.contains("workers = 4"));
    assert!(output.contains("setting = \"preserved\""));
}

#[test]
fn ownership_boundaries_cannot_overlap() {
    let mut registry = ConfigRegistry::new();
    registry.register::<PluginConfig>().unwrap();

    #[derive(Default, Serialize, Deserialize)]
    struct PluginsConfig {}
    impl ConfigSection for PluginsConfig {
        const SECTION: &'static str = "plugins";
    }

    assert!(matches!(
        registry.register::<PluginsConfig>(),
        Err(ConfigError::OverlappingSection { .. })
    ));
}
