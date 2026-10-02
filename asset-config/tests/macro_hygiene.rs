use asset_config::{ConfigError, ConfigSection, Registry, config, register};

type Result<T> = ::core::result::Result<T, &'static str>;
struct Ok;
struct Default;
struct Send;
struct Sync;

#[config(key = "attribute", validate = validate)]
#[derive(::core::default::Default)]
struct AttributeConfig {
    value: u32,
}

fn validate(config: &AttributeConfig) -> Result<()> {
    if config.value == 0 {
        ::core::result::Result::Ok(())
    } else {
        ::core::result::Result::Err("value must be zero")
    }
}

#[config(key = "generic", validate = validate_generic)]
#[derive(::core::default::Default)]
struct GenericConfig<T> {
    value: T,
}

fn validate_generic<T>(_: &GenericConfig<T>) -> Result<()> {
    ::core::result::Result::Ok(())
}

register!(GenericConfig<u32>);

#[test]
fn registrations_and_validation_ignore_shadowed_prelude_names() {
    let _ = (Ok, Default, Send, Sync);
    let loaded = asset_config::load("missing-hygiene-config.toml").unwrap();
    assert_eq!(loaded.get::<AttributeConfig>().unwrap().value, 0);
    assert_eq!(loaded.get::<GenericConfig<u32>>().unwrap().value, 0);

    let error = AttributeConfig { value: 1 }.validate().unwrap_err();
    assert!(matches!(
        error,
        ConfigError::Validation {
            key: "attribute",
            ..
        }
    ));

    let mut registry = Registry::default();
    registry.register::<GenericConfig<u64>>().unwrap();
    let selected = registry.load("missing-hygiene-config.toml").unwrap();
    assert_eq!(selected.get::<GenericConfig<u64>>().unwrap().value, 0);
}
