#![cfg(feature = "derive")]

use ah_config::{ConfigErrorKind as Kind, Registry};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(ah_config::Config)]
#[config(key = "implicit", default = implicit())]
struct Implicit {
    value: u16,
}
fn implicit() -> Implicit {
    Implicit { value: 1 }
}

#[derive(ah_config::Config)]
#[config(key = "shared", spaces = ["default", "plugins"], default = shared())]
struct Shared {
    value: u16,
}
fn shared() -> Shared {
    Shared { value: 2 }
}

#[derive(ah_config::Config)]
#[config(key = "private", spaces = ["plugins"], default = private())]
struct Private {
    value: u16,
}
fn private() -> Private {
    Private { value: 3 }
}

#[test]
fn omitted_space_means_default_and_explicit_space_replaces_it() -> TestResult {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.toml");
    let default = Registry::for_space("default")?.load_optional(&missing)?;
    let plugins = Registry::for_space("plugins")?.load_optional(&missing)?;
    assert_eq!(default.get::<Implicit>()?.value, 1);
    assert_eq!(plugins.get::<Private>()?.value, 3);
    assert_eq!(
        default.get::<Private>().err().map(|error| error.kind()),
        Some(Kind::NotRegistered)
    );
    assert_eq!(
        plugins.get::<Implicit>().err().map(|error| error.kind()),
        Some(Kind::NotRegistered)
    );
    assert_eq!(default.space(), "default");
    assert_eq!(plugins.space(), "plugins");
    assert!(default.path().is_absolute());
    assert_eq!(default.path(), missing);
    assert!(Arc::ptr_eq(
        &default.get::<Shared>()?,
        &default.get::<Shared>()?
    ));
    assert!(!Arc::ptr_eq(
        &default.get::<Shared>()?,
        &plugins.get::<Shared>()?
    ));
    Ok(())
}

#[test]
fn spaces_select_declarations_without_prefixing_file_keys() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(
        &path,
        "[implicit]\nvalue=10\n[shared]\nvalue=20\n[private]\nvalue=30",
    )?;
    let default = Registry::for_space("default")?.load_required(&path)?;
    let plugins = Registry::for_space("plugins")?.load_required(&path)?;
    assert_eq!(default.get::<Implicit>()?.value, 10);
    assert_eq!(default.get::<Shared>()?.value, 20);
    assert_eq!(plugins.get::<Shared>()?.value, 20);
    assert_eq!(plugins.get::<Private>()?.value, 30);
    Ok(())
}

static FACTORIES: AtomicUsize = AtomicUsize::new(0);
static VALIDATIONS: AtomicUsize = AtomicUsize::new(0);
static UNSELECTED: AtomicUsize = AtomicUsize::new(0);

#[derive(ah_config::Config)]
#[config(key = "counter", spaces = ["counted"], default = counted(), validate = validate_counted)]
struct Counted {
    value: u16,
}
fn counted() -> Counted {
    FACTORIES.fetch_add(1, Ordering::SeqCst);
    Counted { value: 8080 }
}
fn validate_counted(_: &Counted) -> Result<(), &'static str> {
    VALIDATIONS.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "unselected", spaces = ["unselected"], default = unselected())]
struct Unselected {
    value: u16,
}
fn unselected() -> Unselected {
    UNSELECTED.fetch_add(1, Ordering::SeqCst);
    Unselected { value: 1 }
}

#[test]
fn factories_and_validators_run_once_per_selected_load_and_get_is_pure() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(
        &path,
        "[counter]\nvalue=42\n[unselected]\nvalue='not an integer'",
    )?;
    let registry = Registry::for_space("counted")?;
    assert_eq!(FACTORIES.load(Ordering::SeqCst), 1);
    assert_eq!(VALIDATIONS.load(Ordering::SeqCst), 0);
    let loaded = registry.load_required(&path)?;
    let first = loaded.get::<Counted>()?;
    for _ in 0..3 {
        assert!(Arc::ptr_eq(&first, &loaded.get::<Counted>()?));
    }
    assert_eq!(first.value, 42);
    assert_eq!(FACTORIES.load(Ordering::SeqCst), 1);
    assert_eq!(VALIDATIONS.load(Ordering::SeqCst), 1);
    assert_eq!(UNSELECTED.load(Ordering::SeqCst), 0);
    let second = Registry::for_space("counted")?.load_required(&path)?;
    assert!(!Arc::ptr_eq(&first, &second.get::<Counted>()?));
    assert_eq!(FACTORIES.load(Ordering::SeqCst), 2);
    assert_eq!(VALIDATIONS.load(Ordering::SeqCst), 2);
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "value", spaces = ["reload"])]
struct Reload {
    value: u16,
}

#[test]
fn snapshots_and_handles_remain_valid_after_files_and_containers_change() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[value]\nvalue=10")?;
    let old = Registry::for_space("reload")?.load_required(&path)?;
    std::fs::write(&path, "[value]\nvalue=20")?;
    let new = Registry::for_space("reload")?.load_required(&path)?;
    let old_handle = old.get::<Reload>()?;
    let new_handle = new.get::<Reload>()?;
    drop(old);
    drop(new);
    std::fs::remove_file(path)?;
    assert_eq!(old_handle.value, 10);
    assert_eq!(new_handle.value, 20);
    let read = std::thread::spawn(move || old_handle.value)
        .join()
        .map_err(|_| std::io::Error::other("reader thread failed"))?;
    assert_eq!(read, 10);
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "same", spaces = ["left"])]
struct Left {
    number: u16,
}
#[derive(ah_config::Config)]
#[config(key = "same", spaces = ["right"])]
struct Right {
    text: String,
}

#[test]
fn same_keys_in_different_spaces_have_independent_types_and_sources() -> TestResult {
    let directory = tempfile::tempdir()?;
    let left_path = directory.path().join("left.toml");
    let right_path = directory.path().join("right.toml");
    std::fs::write(&left_path, "[same]\nnumber=42")?;
    std::fs::write(&right_path, "[same]\ntext='right'")?;
    let left = Registry::for_space("left")?.load_required(left_path)?;
    let right = Registry::for_space("right")?.load_required(right_path)?;
    assert_eq!(left.get::<Left>()?.number, 42);
    assert_eq!(right.get::<Right>()?.text, "right");
    assert_eq!(
        left.get::<Right>().err().map(|error| error.kind()),
        Some(Kind::NotRegistered)
    );
    assert_eq!(
        right.get::<Left>().err().map(|error| error.kind()),
        Some(Kind::NotRegistered)
    );
    Ok(())
}

static CONFLICT_PREPARES: AtomicUsize = AtomicUsize::new(0);
#[derive(ah_config::Config)]
#[config(key = "duplicate", spaces = ["conflict"], default = conflict())]
struct ConflictA {
    value: u16,
}
fn conflict() -> ConflictA {
    CONFLICT_PREPARES.fetch_add(1, Ordering::SeqCst);
    ConflictA { value: 1 }
}
#[derive(ah_config::Config)]
#[config(key = "duplicate", spaces = ["conflict"])]
struct ConflictB {
    value: u16,
}
#[derive(ah_config::Config)]
#[config(key = "parent", spaces = ["overlap"])]
struct Parent {
    value: u16,
}
#[derive(ah_config::Config)]
#[config(key = "parent.child", spaces = ["overlap"])]
struct Child {
    value: u16,
}

#[test]
fn conflicts_are_scoped_and_rejected_before_defaults_are_prepared() {
    assert_eq!(
        Registry::for_space("conflict")
            .err()
            .map(|error| error.kind()),
        Some(Kind::KeyConflict)
    );
    assert_eq!(
        Registry::for_space("overlap")
            .err()
            .map(|error| error.kind()),
        Some(Kind::KeyConflict)
    );
    assert_eq!(CONFLICT_PREPARES.load(Ordering::SeqCst), 0);
}

#[test]
fn unknown_and_invalid_space_names_do_not_silently_select_default() {
    for name in ["missing", "Plugins", "DEFAULT"] {
        assert_eq!(
            Registry::for_space(name).err().map(|error| error.kind()),
            Some(Kind::UnknownSpace)
        );
    }
    for name in ["", " ", "plugins ", "a\0b"] {
        assert_eq!(
            Registry::for_space(name).err().map(|error| error.kind()),
            Some(Kind::InvalidSpace)
        );
    }
}

#[cfg(any())]
#[derive(ah_config::Config)]
#[config(key = "duplicate", spaces = ["conflict"])]
struct DisabledDeclaration {
    value: DoesNotExist,
}
