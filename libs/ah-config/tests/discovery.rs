#![cfg(feature = "derive")]

use ah_config::{ConfigErrorKind, Registry};
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(ah_config::Config)]
#[config(key = "vfs", spaces = ["host", "shared"], default)]
struct Vfs {
    #[config(default = true)]
    enabled: bool,
}

#[derive(ah_config::Config)]
#[config(parent = Vfs, key = "wasm", spaces = ["plugins", "shared"], default)]
struct Wasm {
    #[config(default = 128)]
    memory_mb: u64,
}

static FACTORIES: AtomicUsize = AtomicUsize::new(0);

#[derive(ah_config::Config)]
#[config(key = "metadata_only", spaces = ["metadata"], default = counted())]
struct Counted {}

fn counted() -> Counted {
    FACTORIES.fetch_add(1, Ordering::SeqCst);
    Counted {}
}

#[derive(ah_config::Config)]
#[config(key = "duplicate", spaces = ["conflict"])]
struct DuplicateA {}

#[derive(ah_config::Config)]
#[config(key = "duplicate", spaces = ["conflict"])]
struct DuplicateB {}

#[test]
fn discovery_is_sorted_and_does_not_prepare_defaults() {
    assert_eq!(
        Registry::spaces(),
        ["conflict", "host", "metadata", "plugins", "shared"]
    );
    let declarations = Registry::declarations();
    assert!(declarations.windows(2).all(|pair| (
        pair[0].key(),
        pair[0].type_name(),
        pair[0].spaces()
    ) <= (
        pair[1].key(),
        pair[1].type_name(),
        pair[1].spaces()
    )));
    let wasm = declarations
        .iter()
        .find(|section| section.key() == "vfs.wasm");
    assert!(wasm.is_some_and(
        |section| section.type_name() == std::any::type_name::<Wasm>()
            && section.spaces() == ["plugins", "shared"]
    ));
    assert_eq!(FACTORIES.load(Ordering::SeqCst), 0);
}

#[test]
fn selected_metadata_survives_loading_and_respects_space_membership() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("missing.toml");
    let host = Registry::for_space("host")?;
    assert_eq!(host.sections().len(), 1);
    assert_eq!(host.sections()[0].key(), "vfs");
    let host = host.load_optional(&path)?;
    assert_eq!(host.sections()[0].type_name(), std::any::type_name::<Vfs>());
    assert!(host.get::<Vfs>()?.enabled);

    let plugins = Registry::for_space("plugins")?;
    assert_eq!(plugins.sections().len(), 1);
    assert_eq!(plugins.sections()[0].key(), "vfs.wasm");
    let plugins = plugins.load_optional(&path)?;
    assert_eq!(plugins.get::<Wasm>()?.memory_mb, 128);
    assert!(plugins.get::<Vfs>().is_err());
    Ok(())
}

#[test]
fn conflicting_declarations_remain_visible_for_diagnostics() {
    let declarations = Registry::declarations();
    let duplicates: Vec<_> = declarations
        .iter()
        .filter(|section| section.key() == "duplicate")
        .collect();
    assert_eq!(duplicates.len(), 2);
    assert_ne!(duplicates[0].type_name(), duplicates[1].type_name());
    assert_eq!(
        Registry::for_space("conflict")
            .err()
            .map(|error| error.kind()),
        Some(ConfigErrorKind::KeyConflict)
    );
}

#[test]
fn unknown_spaces_suggest_the_linked_names() -> TestResult {
    let error = Registry::for_space("plugin")
        .err()
        .ok_or("expected unknown space")?;
    assert_eq!(error.available_spaces(), Registry::spaces());
    assert!(error.to_string().contains("available spaces"));
    assert!(error.to_string().contains("plugins"));
    assert!(format!("{error:?}").contains("available_spaces"));
    let invalid = Registry::for_space("")
        .err()
        .ok_or("expected invalid space")?;
    assert!(invalid.available_spaces().is_empty());
    Ok(())
}
