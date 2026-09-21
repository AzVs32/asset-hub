use asset_config::{ConfigError, config, load};

#[config(key = "server..http")]
#[derive(Default)]
struct InvalidKey {
    value: bool,
}

#[test]
fn auto_registration_rejects_invalid_key() {
    assert!(matches!(
        load("missing-config.toml"),
        Err(ConfigError::InvalidKey("server..http"))
    ));
}
