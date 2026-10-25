#![cfg(feature = "derive")]

use ah_config::{ConfigErrorKind, LoadedConfig, Registry};
use std::{
    collections::BTreeMap,
    error::Error,
    sync::{Mutex, RwLock},
};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(ah_config::ConfigValue, Debug, PartialEq)]
#[config(tag = "kind", deny_unknown_fields)]
enum Storage {
    Local {
        #[config(default = "field.sqlite".into())]
        path: String,
    },
    Memory {
        capacity: u64,
        #[config(default = "MiB".into())]
        unit: String,
    },
    Remote {
        url: String,
    },
}

#[derive(ah_config::ConfigValue)]
struct Pool {
    size: u16,
    #[config(rename = "driver")]
    storage: Storage,
}

#[derive(ah_config::ConfigValue)]
struct Wrapped(Storage);

type OptionalStorage = Option<Box<Storage>>;

#[derive(ah_config::Config)]
#[config(key = "service.app", spaces = ["nested"], default = app_defaults())]
struct App {
    pool: Pool,
    optional: OptionalStorage,
    wrapped: Wrapped,
    stores: BTreeMap<String, Storage>,
}

fn app_defaults() -> App {
    App {
        pool: Pool {
            size: 8,
            storage: Storage::Local {
                path: "factory.sqlite".into(),
            },
        },
        optional: Some(Box::new(Storage::Remote {
            url: "https://example.com".into(),
        })),
        wrapped: Wrapped(Storage::Local {
            path: "wrapped.sqlite".into(),
        }),
        stores: BTreeMap::from([
            (
                "main".into(),
                Storage::Local {
                    path: "main.sqlite".into(),
                },
            ),
            (
                "other".into(),
                Storage::Local {
                    path: "other.sqlite".into(),
                },
            ),
        ]),
    }
}

fn load(text: &str) -> Result<LoadedConfig, Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, text)?;
    Ok(Registry::for_space("nested")?.load_required(path)?)
}

#[test]
fn omitted_enums_keep_whole_defaults_while_struct_fields_merge() -> TestResult {
    let app = load("[service.app.pool]\nsize=16")?.get::<App>()?;
    assert_eq!(app.pool.size, 16);
    assert_eq!(
        app.pool.storage,
        Storage::Local {
            path: "factory.sqlite".into()
        }
    );
    assert!(
        matches!(app.optional.as_deref(), Some(Storage::Remote { url }) if url == "https://example.com")
    );
    Ok(())
}

#[test]
fn switching_a_nested_variant_discards_old_fields_and_applies_field_defaults() -> TestResult {
    let app = load("[service.app.pool.driver]\nkind='memory'\ncapacity=512")?.get::<App>()?;
    assert_eq!(app.pool.size, 8);
    assert_eq!(
        app.pool.storage,
        Storage::Memory {
            capacity: 512,
            unit: "MiB".into()
        }
    );
    Ok(())
}

#[test]
fn explicitly_selecting_the_same_variant_uses_its_field_defaults() -> TestResult {
    let app = load("[service.app.pool.driver]\nkind='local'")?.get::<App>()?;
    assert_eq!(
        app.pool.storage,
        Storage::Local {
            path: "field.sqlite".into()
        }
    );
    Ok(())
}

#[test]
fn explicit_enums_require_their_selector_and_required_fields() -> TestResult {
    for text in [
        "[service.app.pool.driver]",
        "[service.app.pool.driver]\npath='custom.sqlite'",
        "[service.app.pool.driver]\nkind='unknown'",
        "[service.app.pool.driver]\nkind='memory'",
        "[service.app.optional]\nkind='remote'",
    ] {
        let error = load(text)
            .err()
            .ok_or("expected invalid enum input to fail")?;
        assert_eq!(
            error
                .downcast_ref::<ah_config::ConfigError>()
                .map(|error| error.kind()),
            Some(ConfigErrorKind::Deserialize),
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn enums_inside_optional_boxes_and_maps_also_replace_defaults() -> TestResult {
    let app = load("[service.app.optional]\nkind='memory'\ncapacity=128\n[service.app.stores.main]\nkind='memory'\ncapacity=256")?.get::<App>()?;
    assert_eq!(
        app.optional.as_deref(),
        Some(&Storage::Memory {
            capacity: 128,
            unit: "MiB".into()
        })
    );
    assert_eq!(
        app.stores.get("main"),
        Some(&Storage::Memory {
            capacity: 256,
            unit: "MiB".into()
        })
    );
    assert_eq!(
        app.stores.get("other"),
        Some(&Storage::Local {
            path: "other.sqlite".into()
        })
    );
    Ok(())
}

#[test]
fn newtype_values_preserve_nested_enum_boundaries() -> TestResult {
    let app = load("[service.app.wrapped]\nkind='memory'\ncapacity=64")?.get::<App>()?;
    assert_eq!(
        app.wrapped.0,
        Storage::Memory {
            capacity: 64,
            unit: "MiB".into()
        }
    );
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "app", spaces = ["locked"], default = locked_defaults())]
struct LockedApp {
    mutex: Mutex<Storage>,
    rwlock: RwLock<Storage>,
    pool: Mutex<Pool>,
}

fn locked_defaults() -> LockedApp {
    LockedApp {
        mutex: Mutex::new(Storage::Local {
            path: "mutex.sqlite".into(),
        }),
        rwlock: RwLock::new(Storage::Remote {
            url: "https://example.com".into(),
        }),
        pool: Mutex::new(Pool {
            size: 8,
            storage: Storage::Local {
                path: "pool.sqlite".into(),
            },
        }),
    }
}

fn load_locked(text: &str) -> Result<LoadedConfig, Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, text)?;
    Ok(Registry::for_space("locked")?.load_required(path)?)
}

#[test]
fn omitted_locked_enums_keep_their_inherited_defaults() -> TestResult {
    let app = load_locked("")?.get::<LockedApp>()?;
    assert_eq!(
        *app.mutex.lock().map_err(|_| "mutex poisoned")?,
        Storage::Local {
            path: "mutex.sqlite".into()
        }
    );
    assert_eq!(
        *app.rwlock.read().map_err(|_| "rwlock poisoned")?,
        Storage::Remote {
            url: "https://example.com".into()
        }
    );
    Ok(())
}

#[test]
fn switching_locked_variants_discards_old_fields() -> TestResult {
    let app = load_locked(
        "[app.mutex]\nkind='memory'\ncapacity=128\n[app.rwlock]\nkind='memory'\ncapacity=256",
    )?
    .get::<LockedApp>()?;
    assert_eq!(
        *app.mutex.lock().map_err(|_| "mutex poisoned")?,
        Storage::Memory {
            capacity: 128,
            unit: "MiB".into()
        }
    );
    assert_eq!(
        *app.rwlock.read().map_err(|_| "rwlock poisoned")?,
        Storage::Memory {
            capacity: 256,
            unit: "MiB".into()
        }
    );
    Ok(())
}

#[test]
fn explicitly_selecting_the_same_locked_variant_uses_its_field_defaults() -> TestResult {
    let app = load_locked("[app.mutex]\nkind='local'")?.get::<LockedApp>()?;
    assert_eq!(
        *app.mutex.lock().map_err(|_| "mutex poisoned")?,
        Storage::Local {
            path: "field.sqlite".into()
        }
    );
    Ok(())
}

#[test]
fn explicit_locked_enums_require_their_selector_and_fields() -> TestResult {
    for text in [
        "[app.mutex]",
        "[app.mutex]\npath='custom.sqlite'",
        "[app.mutex]\nkind='unknown'",
        "[app.mutex]\nkind='memory'",
        "[app.rwlock]",
        "[app.rwlock]\nurl='https://configured.example.com'",
        "[app.rwlock]\nkind='remote'",
        "[app.rwlock]\nkind='memory'",
    ] {
        let error = load_locked(text)
            .err()
            .ok_or("expected invalid locked enum input to fail")?;
        assert_eq!(
            error
                .downcast_ref::<ah_config::ConfigError>()
                .map(|error| error.kind()),
            Some(ConfigErrorKind::Deserialize),
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn locked_structs_merge_fields_and_preserve_deeper_enum_boundaries() -> TestResult {
    let app = load_locked("[app.pool]\nsize=16")?.get::<LockedApp>()?;
    let pool = app.pool.lock().map_err(|_| "mutex poisoned")?;
    assert_eq!(pool.size, 16);
    assert_eq!(
        pool.storage,
        Storage::Local {
            path: "pool.sqlite".into()
        }
    );

    let app = load_locked("[app.pool.driver]\nkind='memory'\ncapacity=64")?.get::<LockedApp>()?;
    let pool = app.pool.lock().map_err(|_| "mutex poisoned")?;
    assert_eq!(pool.size, 8);
    assert_eq!(
        pool.storage,
        Storage::Memory {
            capacity: 64,
            unit: "MiB".into()
        }
    );
    Ok(())
}
