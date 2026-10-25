#![cfg(feature = "derive")]

use ah_config::{ConfigError, ConfigErrorKind, LoadedConfig, Registry};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn load(space: &str, text: &str) -> Result<LoadedConfig, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, text)?;
    Ok(Registry::for_space(space)?.load_required(path)?)
}

fn error_kind(error: Box<dyn std::error::Error>) -> Option<ConfigErrorKind> {
    error.downcast_ref::<ConfigError>().map(ConfigError::kind)
}

#[derive(ah_config::ConfigValue, Debug, PartialEq)]
#[config(default = Self::ReadOnly)]
enum AccessMode {
    ReadOnly,
    ReadWrite,
}

#[derive(ah_config::ConfigValue, Debug, PartialEq)]
enum Unit {
    #[config(rename = "MiB")]
    MiB,
    #[config(rename = "MB")]
    MB,
}

#[derive(ah_config::ConfigValue)]
#[config(deny_unknown_fields)]
struct HttpOptions {
    max_connections: u16,
}

#[derive(ah_config::Config)]
#[config(
    key = "ApiSettings",
    spaces = ["names"],
    default = Self {
        mode: AccessMode::ReadOnly,
        unit: Unit::MiB,
        display_name: "KeepCase".into(),
        http_options: HttpOptions { max_connections: 10 },
    },
    deny_unknown_fields,
)]
struct Settings {
    mode: AccessMode,
    unit: Unit,
    #[config(rename = "displayName")]
    display_name: String,
    http_options: HttpOptions,
}

#[test]
fn fixed_names_and_individual_overrides_apply_to_files_and_defaults() -> TestResult {
    let defaults = load("names", "")?.get::<Settings>()?;
    assert_eq!(defaults.mode, AccessMode::ReadOnly);
    assert_eq!(defaults.unit, Unit::MiB);
    assert_eq!(defaults.display_name, "KeepCase");
    assert_eq!(defaults.http_options.max_connections, 10);

    let configured = load(
        "names",
        r#"
[ApiSettings]
mode = "read_write"
unit = "MB"
displayName = "Keep This Case"
[ApiSettings.http_options]
max_connections = 25
"#,
    )?
    .get::<Settings>()?;
    assert_eq!(configured.mode, AccessMode::ReadWrite);
    assert_eq!(configured.unit, Unit::MB);
    assert_eq!(configured.display_name, "Keep This Case");
    assert_eq!(configured.http_options.max_connections, 25);

    for text in [
        "[ApiSettings]\nmode='ReadWrite'",
        "[ApiSettings]\nunit='mi_b'",
        "[ApiSettings]\ndisplay_name='wrong spelling'",
        "[ApiSettings.http_options]\nmaxConnections=25",
    ] {
        assert_eq!(
            load("names", text).err().and_then(error_kind),
            Some(ConfigErrorKind::Deserialize)
        );
    }
    Ok(())
}

#[derive(ah_config::Config)]
#[config(
    key = "store",
    spaces = ["tagged_names"],
    tag = "DriverKind",
    default = Self::LocalFile,
    deny_unknown_fields,
)]
enum Store {
    LocalFile {
        #[config(default = "example.sqlite".into())]
        file_path: String,
    },
    InMemory {
        max_entries: u64,
        #[config(rename = "readOnly")]
        read_only: bool,
    },
}

#[test]
fn tagged_variants_use_snake_case_while_literal_selectors_stay_unchanged() -> TestResult {
    assert!(matches!(
        &*load("tagged_names", "")?.get::<Store>()?,
        Store::LocalFile { file_path } if file_path == "example.sqlite"
    ));
    assert!(matches!(
        &*load(
            "tagged_names",
            "[store]\nDriverKind='in_memory'\nmax_entries=25\nreadOnly=true"
        )?
        .get::<Store>()?,
        Store::InMemory {
            max_entries: 25,
            read_only: true
        }
    ));
    for text in [
        "[store]\nDriverKind='LocalFile'",
        "[store]\ndriver_kind='local_file'",
        "[store]\nDriverKind='in_memory'\nmax_entries=25\nread_only=true",
    ] {
        assert_eq!(
            load("tagged_names", text).err().and_then(error_kind),
            Some(ConfigErrorKind::Deserialize)
        );
    }
    Ok(())
}
