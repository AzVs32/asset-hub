#![cfg(feature = "derive")]

use ah_config::{ConfigError, ConfigErrorKind as Kind, Registry};
use std::{error::Error, path::Path};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(ah_config::Config)]
#[config(key = "service", spaces = ["namespace"])]
struct Service {}

#[derive(Debug, thiserror::Error)]
#[error("port must be nonzero")]
struct InvalidPort;

#[derive(ah_config::Config)]
#[config(parent = Service, key = "http", spaces = ["diagnostics"], validate = validate_http)]
struct Http {
    port: u16,
}

fn validate_http(config: &Http) -> Result<(), InvalidPort> {
    if config.port == 0 {
        Err(InvalidPort)
    } else {
        Ok(())
    }
}

#[derive(ah_config::Config)]
#[config(parent = Service, key = "http", spaces = ["defaulted"], default = Self { port: 8080 })]
struct DefaultedHttp {
    port: u16,
}

#[derive(ah_config::Config)]
#[config(parent = Service, key = "storage", spaces = ["enum_diagnostics"], tag = "kind")]
enum Storage {
    Memory,
}

#[derive(ah_config::Config)]
#[config(key = "duplicate", spaces = ["diagnostic_conflict"])]
struct ConflictA {}

#[derive(ah_config::Config)]
#[config(key = "duplicate", spaces = ["diagnostic_conflict"])]
struct ConflictB {}

fn failed(
    result: Result<ah_config::LoadedConfig, ConfigError>,
) -> Result<ConfigError, Box<dyn Error>> {
    result
        .err()
        .ok_or_else(|| "expected loading to fail".into())
}

fn assert_context(
    error: &ConfigError,
    kind: Kind,
    path: Option<&Path>,
    section: Option<&str>,
    space: &str,
) {
    assert_eq!(error.kind(), kind);
    assert_eq!(error.path(), path);
    assert_eq!(error.section(), section);
    assert_eq!(error.space(), Some(space));
}

#[test]
fn space_selection_errors_have_no_file_context() -> TestResult {
    for (space, kind) in [
        ("missing-space", Kind::UnknownSpace),
        (" ", Kind::InvalidSpace),
    ] {
        let error = Registry::for_space(space)
            .err()
            .ok_or("expected invalid space")?;
        assert_context(&error, kind, None, None, space);
    }
    let error = Registry::for_space("diagnostic_conflict")
        .err()
        .ok_or("expected a key conflict")?;
    assert_context(
        &error,
        Kind::KeyConflict,
        None,
        Some("duplicate"),
        "diagnostic_conflict",
    );
    Ok(())
}

#[test]
fn file_failures_expose_the_path_and_retain_original_io_errors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("missing.toml");
    let error = failed(Registry::for_space("diagnostics")?.load_required(&path))?;
    assert_context(&error, Kind::MissingFile, Some(&path), None, "diagnostics");
    let cause = error
        .source()
        .and_then(Error::source)
        .and_then(|error| error.downcast_ref::<std::io::Error>())
        .ok_or("missing original I/O error")?;
    assert_eq!(cause.kind(), std::io::ErrorKind::NotFound);

    std::fs::write(&path, "[service.http\n")?;
    let error = failed(Registry::for_space("diagnostics")?.load_required(&path))?;
    assert_context(&error, Kind::Parse, Some(&path), None, "diagnostics");

    let error = failed(Registry::for_space("diagnostics")?.load_required(directory.path()))?;
    assert_context(
        &error,
        Kind::Read,
        Some(directory.path()),
        None,
        "diagnostics",
    );
    Ok(())
}

#[test]
fn section_failures_expose_the_registered_key() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    for (text, kind) in [
        ("", Kind::MissingSection),
        ("[service]", Kind::MissingSection),
        ("service=7", Kind::Deserialize),
        ("[service]\nhttp=7", Kind::Deserialize),
        ("[service.http]\nport='wrong'", Kind::Deserialize),
        ("[service.http]\nport=0", Kind::Validation),
    ] {
        std::fs::write(&path, text)?;
        let error = failed(Registry::for_space("diagnostics")?.load_required(&path))?;
        assert_context(
            &error,
            kind,
            Some(&path),
            Some("service.http"),
            "diagnostics",
        );
        if kind == Kind::Validation {
            assert!(
                error
                    .source()
                    .and_then(Error::source)
                    .is_some_and(|error| error.downcast_ref::<InvalidPort>().is_some())
            );
        }
    }

    std::fs::write(&path, "")?;
    let error = failed(Registry::for_space("enum_diagnostics")?.load_required(&path))?;
    assert_context(
        &error,
        Kind::MissingSection,
        Some(&path),
        Some("service.storage"),
        "enum_diagnostics",
    );
    Ok(())
}

#[test]
fn defaults_do_not_hide_malformed_ancestors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "service=7")?;
    let error = failed(Registry::for_space("defaulted")?.load_required(&path))?;
    assert_context(
        &error,
        Kind::Deserialize,
        Some(&path),
        Some("service.http"),
        "defaulted",
    );
    std::fs::write(&path, "")?;
    assert_eq!(
        Registry::for_space("defaulted")?
            .load_required(&path)?
            .get::<DefaultedHttp>()?
            .port,
        8080
    );
    Ok(())
}

#[test]
fn lookup_errors_include_the_requested_section_and_snapshot_origin() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[service.http]\nport=8080")?;
    let config = Registry::for_space("diagnostics")?.load_required(&path)?;
    let error = config
        .get::<Storage>()
        .err()
        .ok_or("expected an unregistered type")?;
    assert_context(
        &error,
        Kind::NotRegistered,
        Some(&path),
        Some("service.storage"),
        "diagnostics",
    );
    Ok(())
}

#[test]
fn path_resolution_errors_retain_the_requested_path() -> TestResult {
    let error = failed(
        Registry::for_space("diagnostics")?
            .with_base_dir("relative/base")
            .load_optional("config.toml"),
    )?;
    assert_context(
        &error,
        Kind::Read,
        Some(Path::new("config.toml")),
        None,
        "diagnostics",
    );
    Ok(())
}

#[test]
fn printed_decoding_errors_include_locations_without_input_values() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[service.http]\nport='sensitive-input'")?;
    let error = failed(Registry::for_space("diagnostics")?.load_required(&path))?;
    assert_eq!(error.field_path(), Some("service.http.port"));
    for output in [
        format!("{error}"),
        format!("{error:?}"),
        format!("{error:#?}"),
    ] {
        assert!(output.contains("config.toml"));
        assert!(output.contains("service.http.port"));
        assert!(output.contains("diagnostics"));
        assert!(!output.contains("sensitive-input"));
        assert!(!output.contains("invalid type:"));
    }
    assert!(error.source().and_then(Error::source).is_some());
    Ok(())
}

#[test]
fn missing_fields_and_validation_have_distinct_locations() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[service.http]")?;
    let missing = failed(Registry::for_space("diagnostics")?.load_required(&path))?;
    assert_eq!(missing.field_path(), Some("service.http.port"));

    std::fs::write(&path, "[service.http]\nport=0")?;
    let invalid = failed(Registry::for_space("diagnostics")?.load_required(&path))?;
    assert_eq!(invalid.field_path(), None);
    assert!(invalid.to_string().contains("service.http"));
    assert!(!invalid.to_string().contains("port must be nonzero"));
    Ok(())
}

#[derive(ah_config::Config)]
#[config(parent = Service, key = "nested", spaces = ["nested_diagnostics"])]
struct NestedDiagnostics {
    limits: Vec<Limit>,
}

#[derive(ah_config::ConfigValue)]
#[config(deny_unknown_fields)]
struct Limit {
    #[config(rename = "memory_limit_mb")]
    memory: u64,
}

#[test]
fn field_paths_include_nested_names_array_indices_and_missing_fields() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    for (text, field) in [
        (
            "[[service.nested.limits]]\nmemory_limit_mb='secret'",
            "service.nested.limits.0.memory_limit_mb",
        ),
        (
            "[[service.nested.limits]]",
            "service.nested.limits.0.memory_limit_mb",
        ),
        (
            "[[service.nested.limits]]\nmemory_limit_mb=128\nunknown='secret'",
            "service.nested.limits.0.unknown",
        ),
    ] {
        std::fs::write(&path, text)?;
        let error = failed(Registry::for_space("nested_diagnostics")?.load_required(&path))?;
        assert_eq!(error.field_path(), Some(field));
        assert!(!error.to_string().contains("secret"));
    }
    Ok(())
}

#[test]
fn file_and_preparation_errors_print_only_available_context() -> TestResult {
    let error = Registry::for_space("missing-space")
        .err()
        .ok_or("expected an unknown space")?;
    assert!(error.to_string().contains("missing-space"));
    assert!(!error.to_string().contains("file:"));
    assert_eq!(error.field_path(), None);

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("broken.toml");
    std::fs::write(&path, "[sensitive-syntax\n")?;
    let error = failed(Registry::for_space("diagnostics")?.load_required(&path))?;
    for output in [format!("{error}"), format!("{error:?}")] {
        assert!(output.contains("broken.toml"));
        assert!(output.contains("diagnostics"));
        assert!(!output.contains("sensitive-syntax"));
    }
    assert_eq!(error.section(), None);
    assert_eq!(error.field_path(), None);
    Ok(())
}
