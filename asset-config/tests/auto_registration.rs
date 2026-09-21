use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use asset_config::{ConfigError, Registry, config, load};

static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

fn test_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "asset-config-test-{}-{timestamp}-{}.toml",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

#[config(key = "http", validate = validate_http)]
#[derive(Debug)]
struct HttpConfig {
    host: String,
    port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
        }
    }
}

fn validate_http(config: &HttpConfig) -> Result<(), &'static str> {
    if config.port == 0 {
        Err("port must be positive")
    } else {
        Ok(())
    }
}

#[config(key = "cache")]
#[derive(Default)]
struct CacheConfig {
    enabled: bool,
}

#[config(key = "explicit", auto = true)]
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ExplicitAutoConfig {
    enabled: bool,
}

#[config(key = "manual", auto = false)]
#[derive(Default)]
struct ManualConfig {
    enabled: bool,
}

#[config(key = "generic", auto = false)]
#[derive(Default)]
struct GenericConfig<T> {
    value: T,
}

#[test]
fn discovers_sections_and_uses_defaults_without_a_file() {
    let loaded = load(test_path()).unwrap();

    let http = loaded.get::<HttpConfig>().unwrap();
    assert_eq!(http.host, "127.0.0.1");
    assert_eq!(http.port, 8080);
    assert!(!loaded.get::<CacheConfig>().unwrap().enabled);
    assert!(!loaded.get::<ExplicitAutoConfig>().unwrap().enabled);
    assert!(matches!(
        loaded.get::<ManualConfig>(),
        Err(ConfigError::NotRegistered("manual"))
    ));
}

#[test]
fn file_overrides_defaults_and_validation_runs() {
    let path = test_path();
    std::fs::write(&path, "[http]\nport = 9090\n[cache]\nenabled = true\n").unwrap();
    let loaded = load(&path).unwrap();
    std::fs::remove_file(&path).unwrap();

    let http = loaded.get::<HttpConfig>().unwrap();
    assert_eq!(http.host, "127.0.0.1");
    assert_eq!(http.port, 9090);
    assert!(loaded.get::<CacheConfig>().unwrap().enabled);

    std::fs::write(&path, "[http]\nport = 0\n").unwrap();
    let error = load(&path).err().unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        error,
        ConfigError::Validation { key: "http", msg } if msg == "port must be positive"
    ));
}

#[test]
fn manual_registry_can_select_a_section() {
    let mut registry = Registry::default();
    registry.register::<ManualConfig>().unwrap();
    registry.register::<GenericConfig<u32>>().unwrap();
    let loaded = registry.load(test_path()).unwrap();

    assert!(!loaded.get::<ManualConfig>().unwrap().enabled);
    assert_eq!(loaded.get::<GenericConfig<u32>>().unwrap().value, 0);
    assert!(matches!(
        loaded.get::<HttpConfig>(),
        Err(ConfigError::NotRegistered("http"))
    ));
}

#[test]
fn repeated_registration_reports_the_key_conflict() {
    let mut registry = Registry::default();
    registry.register::<ManualConfig>().unwrap();

    assert!(matches!(
        registry.register::<ManualConfig>(),
        Err(ConfigError::KeyConflict {
            existing: "manual",
            incoming: "manual"
        })
    ));
}
