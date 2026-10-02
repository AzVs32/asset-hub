use std::any::type_name;
use std::error::Error;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use asset_config::{ConfigError, config, load, register};

#[config(key = "generic", validate = validate_generic)]
struct GenericConfig<T> {
    value: T,
    port: u16,
}

impl<T: Default> Default for GenericConfig<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            port: 8080,
        }
    }
}

fn validate_generic<T>(config: &GenericConfig<T>) -> Result<(), &'static str> {
    if config.port == 0 {
        Err("port must be positive")
    } else {
        Ok(())
    }
}

register!(GenericConfig<u32>,);

mod provided {
    #[derive(Default, serde::Serialize, serde::Deserialize)]
    pub struct Settings {
        pub enabled: bool,
    }

    impl asset_config::ConfigSection for Settings {
        const KEY: &'static str = "provided";
    }
}

asset_config::register!(provided::Settings);

fn test_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "asset-config-explicit-{}-{timestamp}.toml",
        std::process::id()
    ))
}

#[test]
fn concrete_instances_and_existing_sections_are_loaded_without_modifying_definitions() {
    let loaded = load(test_path()).unwrap();
    let generic = loaded.get::<GenericConfig<u32>>().unwrap();
    assert_eq!(generic.value, 0);
    assert_eq!(generic.port, 8080);
    assert!(!loaded.get::<provided::Settings>().unwrap().enabled);
    assert!(matches!(
        loaded.get::<GenericConfig<u64>>(),
        Err(ConfigError::NotRegistered { type_name: name, key: "generic" })
            if name == type_name::<GenericConfig<u64>>()
    ));
}

#[test]
fn explicit_generic_registration_preserves_file_overrides_and_validation() {
    let path = test_path();
    std::fs::write(
        &path,
        "[generic]\nvalue = 42\nport = 9090\n[provided]\nenabled = true\n",
    )
    .unwrap();
    let loaded = load(&path);
    std::fs::remove_file(&path).unwrap();
    let loaded = loaded.unwrap();
    let generic = loaded.get::<GenericConfig<u32>>().unwrap();
    assert_eq!(generic.value, 42);
    assert_eq!(generic.port, 9090);
    assert!(loaded.get::<provided::Settings>().unwrap().enabled);

    std::fs::write(&path, "[generic]\nport = 0\n").unwrap();
    let result = load(&path);
    std::fs::remove_file(&path).unwrap();
    let error = result.err().unwrap();
    assert!(matches!(
        &error,
        ConfigError::Validation { key: "generic", .. }
    ));
    assert_eq!(error.source().unwrap().to_string(), "port must be positive");
}
