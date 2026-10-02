use asset_config::{ConfigError, Registry, config, load};

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
    for result in [
        Registry::from_auto().map(|_| ()),
        load("missing-config.toml").map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(ConfigError::KeyConflict {
                existing: "server",
                incoming: "server.http"
            })
        ));
    }
}
