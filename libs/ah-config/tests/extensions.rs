#![cfg(feature = "derive")]

use ah_config::{ConfigErrorKind as Kind, Registry};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(ah_config::Config)]
#[config(key = "vfs", spaces = ["extensions"], default)]
struct Vfs {
    #[config(default = true)]
    enabled: bool,
}

#[derive(ah_config::Config)]
#[config(parent = Vfs, key = "rpc", spaces = ["extensions"], default)]
struct Rpc {
    #[config(default = 5000)]
    timeout_ms: u64,
}

#[derive(ah_config::Config)]
#[config(parent = Vfs, key = "wasm", spaces = ["extensions"], default)]
struct Wasm {
    #[config(default = 128)]
    memory_mb: u64,
}

#[derive(ah_config::Config)]
#[config(parent = Wasm, key = "cache", spaces = ["extensions"], default)]
struct Cache {
    #[config(default = 64)]
    capacity: u64,
}

fn load(text: &str) -> Result<ah_config::LoadedConfig, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, text)?;
    Ok(Registry::for_space("extensions")?.load_required(path)?)
}

#[test]
fn parent_and_siblings_keep_their_defaults_and_overrides() -> TestResult {
    let config = load("[vfs]\nenabled=false\n[vfs.wasm]\nmemory_mb=256")?;
    assert!(!config.get::<Vfs>()?.enabled);
    assert_eq!(config.get::<Rpc>()?.timeout_ms, 5000);
    assert_eq!(config.get::<Wasm>()?.memory_mb, 256);
    assert_eq!(config.get::<Cache>()?.capacity, 64);

    let defaults = load("")?;
    assert!(defaults.get::<Vfs>()?.enabled);
    assert_eq!(defaults.get::<Wasm>()?.memory_mb, 128);
    Ok(())
}

#[test]
fn grandchildren_are_loaded_by_their_full_path() -> TestResult {
    let config = load("[vfs.wasm.cache]\ncapacity=512")?;
    assert_eq!(config.get::<Cache>()?.capacity, 512);
    assert_eq!(config.get::<Wasm>()?.memory_mb, 128);
    Ok(())
}

#[test]
fn child_errors_keep_the_full_section_path() -> TestResult {
    let error = load("[vfs.wasm]\nmemory_mb='invalid'")
        .err()
        .ok_or("expected an invalid child field")?;
    let error = error
        .downcast_ref::<ah_config::ConfigError>()
        .ok_or("wrong error type")?;
    assert_eq!(error.kind(), Kind::Deserialize);
    assert_eq!(error.section(), Some("vfs.wasm"));
    Ok(())
}

#[derive(ah_config::Config)]
#[config(parent = Vfs, key = "isolated", spaces = ["child_only"])]
struct Isolated {
    value: u64,
}

#[test]
fn parent_selection_is_independent_of_child_selection() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[vfs.isolated]\nvalue=7")?;
    let config = Registry::for_space("child_only")?.load_required(path)?;
    assert_eq!(config.get::<Isolated>()?.value, 7);
    assert_eq!(
        config.get::<Vfs>().err().map(|error| error.kind()),
        Some(Kind::NotRegistered)
    );
    Ok(())
}

#[derive(ah_config::Config)]
#[config(parent = Vfs, key = "backend", spaces = ["extensions"], tag = "kind", default = Self::Local)]
#[config(deny_unknown_fields)]
enum Backend {
    Local {
        #[config(default = "data".into())]
        path: String,
    },
    Memory,
}

#[test]
fn child_enum_replaces_its_defaults_without_affecting_parent() -> TestResult {
    let config = load("[vfs.backend]\nkind='memory'")?;
    assert!(matches!(&*config.get::<Backend>()?, Backend::Memory));
    assert!(config.get::<Vfs>()?.enabled);
    assert!(matches!(&*load("")?.get::<Backend>()?, Backend::Local { path } if path == "data"));
    Ok(())
}
