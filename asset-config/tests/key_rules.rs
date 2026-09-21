use asset_config::{ConfigError, ConfigSection, Registry, config};
use std::time::{SystemTime, UNIX_EPOCH};

macro_rules! section {
    ($name:ident, $key:literal) => {
        #[config(key = $key, auto = false)]
        #[derive(Default)]
        struct $name {
            value: bool,
        }
    };
}

section!(Server, "server");
section!(Http, "server.http");
section!(Grpc, "server.grpc");
section!(Http2, "server.http2");
section!(HttpUnderscore, "server.http_api");
section!(HttpDash, "server.http-api");
section!(UpperServer, "Server");
section!(EmptyKey, "  ");

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ArrayPath {
    value: bool,
}

impl ConfigSection for ArrayPath {
    const KEY: &'static str = "server[0]";
}

#[test]
fn parent_and_child_conflict_in_both_registration_orders() {
    let mut registry = Registry::default();
    registry.register::<Server>().unwrap();
    assert!(matches!(
        registry.register::<Http>(),
        Err(ConfigError::KeyConflict {
            existing: "server",
            incoming: "server.http"
        })
    ));

    let mut registry = Registry::default();
    registry.register::<Http>().unwrap();
    assert!(matches!(
        registry.register::<Server>(),
        Err(ConfigError::KeyConflict {
            existing: "server.http",
            incoming: "server"
        })
    ));
}

#[test]
fn siblings_and_distinct_spellings_can_coexist() {
    let mut registry = Registry::default();
    registry.register::<Http>().unwrap();
    registry.register::<Grpc>().unwrap();
    registry.register::<Http2>().unwrap();
    registry.register::<HttpUnderscore>().unwrap();
    registry.register::<HttpDash>().unwrap();
    registry.register::<UpperServer>().unwrap();

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "asset-config-key-rules-{}-{timestamp}.toml",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "[server.http]\nvalue = true\n[server.http_api]\nvalue = true\n[server.http-api]\nvalue = true\n[Server]\nvalue = true\n",
    )
    .unwrap();
    let loaded = registry.load(&path).unwrap();
    std::fs::remove_file(path).unwrap();

    assert!(loaded.get::<Http>().unwrap().value);
    assert!(!loaded.get::<Grpc>().unwrap().value);
    assert!(!loaded.get::<Http2>().unwrap().value);
    assert!(loaded.get::<HttpUnderscore>().unwrap().value);
    assert!(loaded.get::<HttpDash>().unwrap().value);
    assert!(loaded.get::<UpperServer>().unwrap().value);
}

#[test]
fn manual_sections_reject_non_bare_paths() {
    let mut registry = Registry::default();
    assert!(matches!(
        registry.register::<ArrayPath>(),
        Err(ConfigError::InvalidKey("server[0]"))
    ));
    assert!(matches!(
        registry.register::<EmptyKey>(),
        Err(ConfigError::InvalidKey("  "))
    ));
}
