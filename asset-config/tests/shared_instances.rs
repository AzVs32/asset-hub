use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use asset_config::{ConfigError, Registry, config};
use serde::{Deserialize, Deserializer};

static DESERIALIZATIONS: AtomicUsize = AtomicUsize::new(0);
static DROPS: AtomicUsize = AtomicUsize::new(0);

#[config(key = "counted", auto = false, validate = validate_counted)]
#[derive(Default)]
struct CountedConfig {
    #[serde(deserialize_with = "deserialize_counted")]
    value: u32,
    #[serde(skip)]
    validations: AtomicUsize,
}

fn deserialize_counted<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    DESERIALIZATIONS.fetch_add(1, Ordering::SeqCst);
    u32::deserialize(deserializer)
}

fn validate_counted(config: &CountedConfig) -> Result<(), &'static str> {
    config.validations.fetch_add(1, Ordering::SeqCst);
    if config.value == 0 {
        Err("value must be positive")
    } else {
        Ok(())
    }
}

fn test_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "asset-config-shared-{}-{timestamp}.toml",
        std::process::id()
    ))
}

#[test]
fn each_load_parses_once_and_all_reads_share_the_actual_validated_instance() {
    let path = test_path();
    std::fs::write(&path, "[counted]\nvalue = 42\n").unwrap();
    let mut registry = Registry::default();
    registry.register::<CountedConfig>().unwrap();
    assert_eq!(DESERIALIZATIONS.load(Ordering::SeqCst), 0);
    let loaded = registry.load(&path).unwrap();
    assert_eq!(DESERIALIZATIONS.load(Ordering::SeqCst), 1);

    let first: Arc<CountedConfig> = loaded.get().unwrap();
    assert_eq!(first.value, 42);
    assert_eq!(first.validations.load(Ordering::SeqCst), 1);
    std::thread::scope(|scope| {
        let readers: Vec<_> = (0..16)
            .map(|_| scope.spawn(|| loaded.get::<CountedConfig>().unwrap()))
            .collect();
        for reader in readers {
            let value = reader.join().unwrap();
            assert!(Arc::ptr_eq(&first, &value));
            assert_eq!(value.validations.load(Ordering::SeqCst), 1);
        }
    });
    assert_eq!(DESERIALIZATIONS.load(Ordering::SeqCst), 1);

    std::fs::write(&path, "[counted]\nvalue = 7\n").unwrap();
    let mut registry = Registry::default();
    registry.register::<CountedConfig>().unwrap();
    let reloaded = registry.load(&path).unwrap();
    let second = reloaded.get::<CountedConfig>().unwrap();
    assert_eq!(DESERIALIZATIONS.load(Ordering::SeqCst), 2);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(first.value, 42);
    assert_eq!(second.value, 7);
    assert_eq!(second.validations.load(Ordering::SeqCst), 1);

    std::fs::write(&path, "[counted]\nvalue = 0\n").unwrap();
    let mut registry = Registry::default();
    registry.register::<CountedConfig>().unwrap();
    let failed = registry.load(&path);
    std::fs::remove_file(path).unwrap();
    let error = failed.err().unwrap();
    assert!(matches!(
        &error,
        ConfigError::Validation { key: "counted", .. }
    ));
    assert_eq!(
        error.source().unwrap().to_string(),
        "value must be positive"
    );
    assert_eq!(DESERIALIZATIONS.load(Ordering::SeqCst), 3);

    drop(loaded);
    drop(reloaded);
    assert_eq!(first.value, 42);
    assert_eq!(second.value, 7);
}

#[config(key = "tracked", auto = false, validate = track_validation)]
#[derive(Default)]
struct TrackedConfig {
    value: bool,
    #[serde(skip)]
    validated: AtomicBool,
}

fn track_validation(config: &TrackedConfig) -> Result<(), &'static str> {
    config.validated.store(true, Ordering::SeqCst);
    Ok(())
}

impl Drop for TrackedConfig {
    fn drop(&mut self) {
        if self.validated.load(Ordering::SeqCst) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[config(key = "rejected", auto = false, validate = reject)]
#[derive(Default)]
struct RejectedConfig {
    value: bool,
}

fn reject(_: &RejectedConfig) -> Result<(), &'static str> {
    Err("rejected")
}

#[test]
fn a_later_validation_failure_drops_previously_validated_instances() {
    let mut registry = Registry::default();
    registry.register::<TrackedConfig>().unwrap();
    registry.register::<RejectedConfig>().unwrap();
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);

    let error = registry.load(test_path()).err().unwrap();
    assert!(matches!(
        &error,
        ConfigError::Validation {
            key: "rejected",
            ..
        }
    ));
    assert_eq!(error.source().unwrap().to_string(), "rejected");
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
}
