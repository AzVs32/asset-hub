use asset_config::{ConfigError, config, load};

#[config(key = "server..http")]
#[derive(Default)]
struct InvalidKey<T> {
    value: T,
}

asset_config::register!(InvalidKey<u32>);

#[test]
fn explicit_registration_rejects_invalid_keys() {
    assert!(matches!(
        load("missing-config.toml"),
        Err(ConfigError::InvalidKey("server..http"))
    ));
}
