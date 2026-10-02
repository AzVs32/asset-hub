use asset_config::{ConfigError, Registry, config, load};
use std::any::type_name;

#[config(key = "enabled")]
#[cfg(test)]
#[cfg_attr(test, cfg_attr(test, cfg(test), derive(Default, Debug)))]
struct Enabled {
    value: bool,
}

#[config(key = "enabled")]
#[cfg(any())]
struct DisabledWithoutDefault;

#[config(key = "enabled")]
#[cfg_attr(all(), cfg(any()), derive(Default))]
struct DisabledByCfgAttr;

#[config(key = "enabled")]
#[cfg_attr(all(), cfg_attr(all(), cfg(any()), derive(Debug)))]
struct DisabledByNestedCfgAttr;

#[config(key = "disabled_generic")]
#[cfg(any())]
struct DisabledManual<T> {
    value: T,
}

#[config(key = "manual")]
#[cfg_attr(test, cfg(test), derive(Default))]
struct Manual<T> {
    value: T,
}

#[test]
fn only_enabled_sections_are_registered_and_manual_generics_remain_usable() {
    let path = std::env::temp_dir().join(format!(
        "asset-config-conditional-{}-missing.toml",
        std::process::id()
    ));
    let loaded = load(&path).unwrap();
    assert!(!loaded.get::<Enabled>().unwrap().value);
    assert!(matches!(
        loaded.get::<Manual<u32>>(),
        Err(ConfigError::NotRegistered { type_name: name, key: "manual" })
            if name == type_name::<Manual<u32>>()
    ));

    let mut registry = Registry::default();
    registry.register::<Manual<u32>>().unwrap();
    let loaded = registry.load(path).unwrap();
    assert_eq!(loaded.get::<Manual<u32>>().unwrap().value, 0);
}
