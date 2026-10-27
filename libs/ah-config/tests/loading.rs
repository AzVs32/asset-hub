#![cfg(feature = "derive")]

use ah_config::{ConfigError, ConfigErrorKind as Kind, Registry};
use std::{
    error::Error,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

type TestResult = Result<(), Box<dyn Error>>;

fn load(space: &str, text: &str) -> Result<ah_config::LoadedConfig, Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, text)?;
    Ok(Registry::for_space(space)?.load_required(path)?)
}

fn kind(result: Result<ah_config::LoadedConfig, Box<dyn Error>>) -> Option<Kind> {
    result
        .err()
        .and_then(|error| error.downcast_ref::<ConfigError>().map(ConfigError::kind))
}

#[derive(ah_config::Config)]
#[config(key = "service", spaces = ["namespace"])]
struct Service {}

#[derive(ah_config::Config, Default)]
#[config(parent = Service, key = "http", spaces = ["required"])]
#[config(deny_unknown_fields)]
struct Required {
    port: u16,
    path: PathBuf,
}

#[test]
fn required_sections_do_not_implicitly_use_default_and_dotted_keys_work() -> TestResult {
    assert_eq!(kind(load("required", "")), Some(Kind::MissingSection));
    let loaded = load(
        "required",
        "[service.http]\nport=8080\npath='relative/data'",
    )?;
    let config = loaded.get::<Required>()?;
    assert_eq!(config.port, 8080);
    assert_eq!(config.path, PathBuf::from("relative/data"));
    assert!(loaded.path().is_absolute());
    Ok(())
}

#[test]
fn malformed_section_ancestors_are_not_reported_as_missing() {
    for text in [
        "service=7",
        "service='wrong'",
        "service=[]",
        "service=false",
    ] {
        assert_eq!(
            kind(load("required", text)),
            Some(Kind::Deserialize),
            "{text}"
        );
    }
    for text in ["", "[service]", "[service.other]\nport=8080"] {
        assert_eq!(
            kind(load("required", text)),
            Some(Kind::MissingSection),
            "{text}"
        );
    }
}

#[test]
fn extraction_is_strict_and_unknown_fields_follow_serde() {
    for port in ["'8080'", "true", "12.6", "-1", "65536"] {
        assert_eq!(
            kind(load(
                "required",
                &format!("[service.http]\nport={port}\npath='x'")
            )),
            Some(Kind::Deserialize)
        );
    }
    assert_eq!(
        kind(load(
            "required",
            "[service.http]\nport=8080\npath='x'\nunknown=1"
        )),
        Some(Kind::Deserialize)
    );
    assert_eq!(
        kind(load("required", "[service.http]\nport=8080")),
        Some(Kind::Deserialize)
    );
}

#[derive(ah_config::ConfigValue)]
struct Nested {
    left: u16,
    right: u16,
}
#[derive(ah_config::Config)]
#[config(key = "settings", spaces = ["struct_defaults"], default = struct_defaults())]
struct StructDefaults {
    count: u16,
    nested: Nested,
    list: Vec<u16>,
    optional: Option<String>,
}
fn struct_defaults() -> StructDefaults {
    StructDefaults {
        count: 12,
        nested: Nested { left: 1, right: 2 },
        list: vec![1, 2],
        optional: None,
    }
}

#[test]
fn structs_merge_fields_recursively_and_arrays_replace() -> TestResult {
    let loaded = load(
        "struct_defaults",
        "[settings]\nlist=[]\noptional='set'\n[settings.nested]\nleft=9",
    )?;
    let config = loaded.get::<StructDefaults>()?;
    assert_eq!(config.count, 12);
    assert_eq!(config.nested.left, 9);
    assert_eq!(config.nested.right, 2);
    assert!(config.list.is_empty());
    assert_eq!(config.optional.as_deref(), Some("set"));
    let defaults = load("struct_defaults", "")?.get::<StructDefaults>()?;
    assert_eq!(defaults.list, [1, 2]);
    assert_eq!(defaults.optional, None);
    Ok(())
}

#[test]
fn optional_only_tolerates_missing_files_and_still_requires_sections() -> TestResult {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.toml");
    assert_eq!(
        Registry::for_space("struct_defaults")?
            .load_required(&missing)
            .err()
            .map(|error| error.kind()),
        Some(Kind::MissingFile)
    );
    assert_eq!(
        Registry::for_space("struct_defaults")?
            .load_optional(&missing)?
            .get::<StructDefaults>()?
            .count,
        12
    );
    assert_eq!(
        Registry::for_space("required")?
            .load_optional(&missing)
            .err()
            .map(|error| error.kind()),
        Some(Kind::MissingSection)
    );
    assert_eq!(
        Registry::for_space("struct_defaults")?
            .load_optional(directory.path())
            .err()
            .map(|error| error.kind()),
        Some(Kind::Read)
    );
    let bad = directory.path().join("bad.toml");
    std::fs::write(&bad, "[unselected\n")?;
    assert_eq!(
        Registry::for_space("struct_defaults")?
            .load_optional(&bad)
            .err()
            .map(|error| error.kind()),
        Some(Kind::Parse)
    );
    std::fs::write(&bad, "[settings]\ncount='wrong'")?;
    assert_eq!(
        Registry::for_space("struct_defaults")?
            .load_optional(&bad)
            .err()
            .map(|error| error.kind()),
        Some(Kind::Deserialize)
    );
    std::fs::write(&bad, [0xff])?;
    assert_eq!(
        Registry::for_space("struct_defaults")?
            .load_optional(&bad)
            .err()
            .map(|error| error.kind()),
        Some(Kind::Read)
    );
    Ok(())
}

#[test]
fn missing_optional_files_return_a_warning_after_validating_defaults() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    let defaults = Registry::for_space("struct_defaults")?.load_optional(&path)?;
    assert_eq!(defaults.get::<StructDefaults>()?.count, 12);
    assert!(
        matches!(defaults.warnings(), [ah_config::ConfigWarning::MissingFile { path: missing }] if missing == &path)
    );
    assert!(
        defaults.warnings()[0]
            .to_string()
            .contains("using declared defaults")
    );

    // An existing empty file is different from a missing optional file.
    std::fs::write(&path, "")?;
    let present = Registry::for_space("struct_defaults")?.load_optional(&path)?;
    assert!(present.warnings().is_empty());
    assert_eq!(defaults.warnings().len(), 1);
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "database", spaces = ["database"], default = database_defaults())]
#[config(tag = "backend", deny_unknown_fields)]
enum Database {
    Sqlite {
        #[config(default = serde_path())]
        path: String,
    },
    Postgres {
        url: String,
    },
    Memory,
}
fn serde_path() -> String {
    "serde.sqlite".into()
}
fn database_defaults() -> Database {
    Database::Sqlite {
        path: "factory.sqlite".into(),
    }
}

#[test]
fn enum_sections_replace_defaults_even_within_the_same_variant() -> TestResult {
    assert!(
        matches!(&*load("database", "")?.get::<Database>()?, Database::Sqlite { path } if path == "factory.sqlite")
    );
    assert!(
        matches!(&*load("database", "[database]\nbackend='sqlite'")?.get::<Database>()?, Database::Sqlite { path } if path == "serde.sqlite")
    );
    assert!(matches!(
        &*load("database", "[database]\nbackend='memory'")?.get::<Database>()?,
        Database::Memory
    ));
    assert!(
        matches!(&*load("database", "[database]\nbackend='postgres'\nurl='postgres://example'")?.get::<Database>()?, Database::Postgres { url } if url == "postgres://example")
    );
    for text in [
        "[database]",
        "[database]\nbackend='unknown'",
        "[database]\nbackend='postgres'",
        "[database]\nbackend='sqlite'\nunknown=1",
    ] {
        assert_eq!(kind(load("database", text)), Some(Kind::Deserialize));
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("port must be nonzero")]
struct InvalidPort;

#[derive(ah_config::Config)]
#[config(key = "http", spaces = ["validation"], default = valid_defaults(), validate = validate_port)]
struct Validated {
    port: u16,
}
fn valid_defaults() -> Validated {
    Validated { port: 8080 }
}
fn validate_port(config: &Validated) -> Result<(), InvalidPort> {
    if config.port == 0 {
        Err(InvalidPort)
    } else {
        Ok(())
    }
}

#[test]
fn validation_errors_retain_typed_causes_and_do_not_fall_back() -> TestResult {
    let error = load("validation", "[http]\nport=0")
        .err()
        .ok_or("expected validation error")?;
    assert_eq!(
        error.downcast_ref::<ConfigError>().map(ConfigError::kind),
        Some(Kind::Validation)
    );
    let mut source = error.source();
    let mut found = false;
    while let Some(cause) = source {
        found |= cause.downcast_ref::<InvalidPort>().is_some();
        source = cause.source();
    }
    assert!(found);
    assert_eq!(load("validation", "")?.get::<Validated>()?.port, 8080);
    Ok(())
}

#[derive(serde::Deserialize)]
struct BrokenValue {
    value: String,
}
impl serde::Serialize for BrokenValue {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom(&self.value))
    }
}

#[derive(ah_config::Config)]
#[config(key = "secret", spaces = ["bad_defaults"], default = bad_defaults())]
struct BadDefaults {
    value: BrokenValue,
}
fn bad_defaults() -> BadDefaults {
    BadDefaults {
        value: BrokenValue {
            value: "sensitive-default".into(),
        },
    }
}

#[test]
fn invalid_defaults_have_typed_errors_without_disclosing_values_in_display_or_debug() -> TestResult
{
    let error = Registry::for_space("bad_defaults")
        .err()
        .ok_or("expected defaults error")?;
    assert_eq!(error.kind(), Kind::Defaults);
    assert_eq!(error.path(), None);
    assert_eq!(error.section(), Some("secret"));
    assert_eq!(error.space(), Some("bad_defaults"));
    assert!(!format!("{error}").contains("sensitive-default"));
    assert!(!format!("{error:?}").contains("sensitive-default"));
    let diagnostic = error
        .source()
        .and_then(Error::source)
        .ok_or("cause missing")?
        .to_string();
    assert!(diagnostic.contains("sensitive-default"));
    Ok(())
}

static DROPPED: AtomicUsize = AtomicUsize::new(0);
#[derive(ah_config::Config)]
#[config(key = "a", spaces = ["partial"])]
struct First {
    value: u16,
}
impl Drop for First {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}
#[derive(ah_config::Config)]
#[config(key = "z", spaces = ["partial"], validate = fail_last)]
struct Last {
    value: u16,
}
fn fail_last(_: &Last) -> Result<(), &'static str> {
    Err("rejected")
}

#[test]
fn a_late_failure_discards_previously_loaded_instances() {
    assert_eq!(
        kind(load("partial", "[a]\nvalue=1\n[z]\nvalue=2")),
        Some(Kind::Validation)
    );
    assert_eq!(DROPPED.load(Ordering::SeqCst), 1);
}
