use asset_config::{ConfigError, config, load};

#[config(key = "duplicate")]
#[derive(Default)]
struct First {
    value: bool,
}

#[config(key = "duplicate")]
#[derive(Default)]
struct Second<T> {
    value: T,
}

asset_config::register!(Second<u32>);

#[test]
fn explicit_and_attribute_registration_reject_duplicate_keys() {
    assert!(matches!(
        load("missing-config.toml"),
        Err(ConfigError::KeyConflict {
            existing: "duplicate",
            incoming: "duplicate"
        })
    ));
}
