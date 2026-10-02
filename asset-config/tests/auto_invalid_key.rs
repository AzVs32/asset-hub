use asset_config::{ConfigError, Registry, config, load};

#[config(key = "server..http")]
#[derive(Default)]
struct InvalidKey<T> {
    value: T,
}

asset_config::register!(InvalidKey<u32>);

#[test]
fn explicit_registration_rejects_invalid_keys() {
    for result in [
        Registry::from_auto().map(|_| ()),
        load("missing-config.toml").map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(ConfigError::InvalidKey("server..http"))
        ));
    }
}
