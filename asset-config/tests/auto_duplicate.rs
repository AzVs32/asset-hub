use asset_config::{ConfigError, config, load};

#[config(key = "duplicate")]
#[derive(Default)]
struct First {
    value: bool,
}

#[config(key = "duplicate")]
#[derive(Default)]
struct Second {
    value: bool,
}

#[test]
fn duplicate_auto_keys_are_rejected() {
    assert!(matches!(
        load("missing-config.toml"),
        Err(ConfigError::DuplicateKey("duplicate"))
    ));
}
