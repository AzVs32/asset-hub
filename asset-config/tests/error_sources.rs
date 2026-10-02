use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use asset_config::{ConfigError, ConfigSection, Registry, config};

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
#[error("user validation failed")]
struct UserError {
    #[source]
    cause: std::io::Error,
}

#[config(key = "borrowed", auto = false, validate = validate_borrowed)]
struct BorrowedConfig {
    reason: String,
}

impl Default for BorrowedConfig {
    fn default() -> Self {
        Self {
            reason: "borrowed message".into(),
        }
    }
}

fn validate_borrowed(config: &BorrowedConfig) -> Result<(), &str> {
    Err(config.reason.as_str())
}

#[config(key = "owned", auto = false, validate = validate_owned)]
#[derive(Default)]
struct OwnedConfig {
    value: bool,
}

fn validate_owned(_: &OwnedConfig) -> Result<(), String> {
    Err("owned message".into())
}

#[config(key = "typed", auto = false, validate = validate_typed)]
#[derive(Default)]
struct TypedConfig {
    value: bool,
}

fn validate_typed(_: &TypedConfig) -> Result<(), UserError> {
    Err(UserError {
        cause: std::io::Error::other("typed cause"),
    })
}

#[config(key = "boxed", auto = false, validate = validate_boxed)]
#[derive(Default)]
struct BoxedConfig {
    value: bool,
}

fn validate_boxed(_: &BoxedConfig) -> Result<(), BoxError> {
    Err(Box::new(UserError {
        cause: std::io::Error::other("boxed cause"),
    }))
}

fn validation_error<T: ConfigSection>() -> ConfigError {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "asset-config-error-sources-{}-{timestamp}.toml",
        std::process::id()
    ));
    let mut registry = Registry::default();
    registry.register::<T>().unwrap();
    registry.load(path).err().unwrap()
}

#[test]
fn borrowed_and_owned_string_validation_errors_become_owned_sources() {
    for (error, key, message) in [
        (
            validation_error::<BorrowedConfig>(),
            "borrowed",
            "borrowed message",
        ),
        (validation_error::<OwnedConfig>(), "owned", "owned message"),
    ] {
        assert!(matches!(
            &error,
            ConfigError::Validation { key: actual, .. } if *actual == key
        ));
        assert_eq!(error.to_string(), format!("invalid config `{key}`"));
        assert_eq!(error.source().unwrap().to_string(), message);
    }
}

#[test]
fn typed_and_boxed_validation_errors_keep_the_original_type_and_nested_source() {
    for (error, key, cause) in [
        (validation_error::<TypedConfig>(), "typed", "typed cause"),
        (validation_error::<BoxedConfig>(), "boxed", "boxed cause"),
    ] {
        assert!(matches!(
            &error,
            ConfigError::Validation { key: actual, .. } if *actual == key
        ));
        let source = error.source().unwrap();
        assert!(source.downcast_ref::<UserError>().is_some());
        assert_eq!(source.to_string(), "user validation failed");
        let cause_error = source.source().unwrap();
        assert!(cause_error.downcast_ref::<std::io::Error>().is_some());
        assert_eq!(cause_error.to_string(), cause);
        let report = format!("{error}: {source}: {cause_error}");
        assert_eq!(report.matches("user validation failed").count(), 1);
        assert_eq!(report.matches(cause).count(), 1);
    }
}
