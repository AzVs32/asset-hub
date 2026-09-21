use asset_config::{ConfigError, config, load};

#[config(key = "server")]
#[derive(Default)]
struct Server {
    value: bool,
}

#[config(key = "server.http")]
#[derive(Default)]
struct Http {
    value: bool,
}

#[test]
fn auto_registration_rejects_parent_and_child_keys() {
    assert!(matches!(
        load("missing-config.toml"),
        Err(ConfigError::KeyConflict {
            existing: "server",
            incoming: "server.http"
        })
    ));
}
