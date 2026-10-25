#![cfg(feature = "derive")]

use ah_config::{ConfigErrorKind as Kind, Registry};
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn load(space: &str, text: &str) -> Result<ah_config::LoadedConfig, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, text)?;
    Ok(Registry::for_space(space)?.load_required(path)?)
}

fn config_kind(error: Box<dyn std::error::Error>) -> Option<Kind> {
    error
        .downcast_ref::<ah_config::ConfigError>()
        .map(|error| error.kind())
}

#[derive(ah_config::ConfigValue, Debug, PartialEq)]
#[config(default = Self::Safe)]
enum Mode {
    Safe,
    Fast,
}

#[derive(ah_config::ConfigValue)]
#[config(default)]
struct Retry {
    #[config(default = 3)]
    attempts: u16,
    #[config(default = 100)]
    delay_ms: u64,
}

static FIELD_FACTORIES: AtomicUsize = AtomicUsize::new(0);
fn default_label() -> String {
    FIELD_FACTORIES.fetch_add(1, Ordering::SeqCst);
    "generated".into()
}

#[derive(ah_config::Config)]
#[config(key = "service", spaces = ["fields"])]
struct Fields {
    #[config(default = "127.0.0.1".into())]
    host: String,
    #[config(default = Self::DEFAULT_PORT)]
    port: u16,
    #[config(default)]
    mode: Mode,
    #[config(default)]
    retry: Retry,
    #[config(default)]
    tags: Vec<String>,
    #[config(default = default_label())]
    label: String,
    optional: Option<String>,
}

impl Fields {
    const DEFAULT_PORT: u16 = 8080;
}

#[test]
fn missing_sections_and_fields_use_only_declared_default_sources() -> TestResult {
    let config = load("fields", "")?.get::<Fields>()?;
    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 8080);
    assert_eq!(config.mode, Mode::Safe);
    assert_eq!(config.retry.attempts, 3);
    assert_eq!(config.retry.delay_ms, 100);
    assert!(config.tags.is_empty());
    assert_eq!(config.label, "generated");
    assert_eq!(config.optional, None);
    assert_eq!(FIELD_FACTORIES.load(Ordering::SeqCst), 1);

    let loaded = load(
        "fields",
        "[service]\nhost='localhost'\nport=9000\nmode='fast'\nlabel='explicit'\n[service.retry]\nattempts=5",
    )?;
    let config = loaded.get::<Fields>()?;
    assert_eq!(config.host, "localhost");
    assert_eq!(config.port, 9000);
    assert_eq!(config.mode, Mode::Fast);
    assert_eq!(config.retry.attempts, 5);
    assert_eq!(config.retry.delay_ms, 100);
    assert_eq!(FIELD_FACTORIES.load(Ordering::SeqCst), 1);
    assert!(std::sync::Arc::ptr_eq(&config, &loaded.get::<Fields>()?));
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "service", spaces = ["strict_fields"])]
struct StrictFields {
    required: u16,
    #[config(default = 8080)]
    port: u16,
}

impl Default for StrictFields {
    fn default() -> Self {
        Self {
            required: 42,
            port: 1234,
        }
    }
}

#[test]
fn ordinary_default_implementations_do_not_make_fields_optional() -> TestResult {
    assert_eq!(StrictFields::default().required, 42);
    assert_eq!(StrictFields::default().port, 1234);
    assert_eq!(
        load("strict_fields", "").err().and_then(config_kind),
        Some(Kind::MissingSection)
    );
    assert_eq!(
        load("strict_fields", "[service]\nport=80")
            .err()
            .and_then(config_kind),
        Some(Kind::Deserialize)
    );
    let config = load("strict_fields", "[service]\nrequired=1")?.get::<StrictFields>()?;
    assert_eq!(config.required, 1);
    assert_eq!(config.port, 8080);
    for value in ["'wrong'", "-1", "70000"] {
        assert_eq!(
            load(
                "strict_fields",
                &format!("[service]\nrequired=1\nport={value}")
            )
            .err()
            .and_then(config_kind),
            Some(Kind::Deserialize)
        );
    }
    Ok(())
}

#[derive(ah_config::ConfigValue, Debug, PartialEq)]
#[config(default = Self::MiB)]
enum Unit {
    #[config(rename = "MB")]
    MB,
    #[config(rename = "MiB")]
    MiB,
}

#[derive(ah_config::Config)]
#[config(key = "database", spaces = ["variant"], tag = "backend")]
#[config(default = Self::Sqlite)]
enum Database {
    Sqlite {
        #[config(default = "default.sqlite".into())]
        path: String,
        optional: Option<String>,
    },
    Memory {
        capacity: u64,
        #[config(default)]
        unit: Unit,
    },
    #[config(rename = "pg")]
    Postgres { url: String },
}

#[test]
fn default_variants_supply_sections_but_never_missing_selectors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let missing = directory.path().join("missing.toml");
    assert_eq!(
        Registry::for_space("variant")?
            .load_required(&missing)
            .err()
            .map(|error| error.kind()),
        Some(Kind::MissingFile)
    );
    assert!(matches!(
        &*Registry::for_space("variant")?
            .load_optional(&missing)?
            .get::<Database>()?,
        Database::Sqlite { path, .. } if path == "default.sqlite"
    ));
    assert!(
        matches!(Database::default(), Database::Sqlite { path, optional: None } if path == "default.sqlite")
    );
    assert_eq!(Unit::default(), Unit::MiB);
    assert!(
        matches!(&*load("variant", "")?.get::<Database>()?, Database::Sqlite { path, optional: None } if path == "default.sqlite")
    );
    assert!(
        matches!(&*load("variant", "[database]\nbackend='sqlite'")?.get::<Database>()?, Database::Sqlite { path, .. } if path == "default.sqlite")
    );
    assert!(matches!(
        &*load("variant", "[database]\nbackend='memory'\ncapacity=512")?.get::<Database>()?,
        Database::Memory {
            capacity: 512,
            unit: Unit::MiB
        }
    ));
    assert!(matches!(
        &*load(
            "variant",
            "[database]\nbackend='memory'\ncapacity=1\nunit='MB'"
        )?
        .get::<Database>()?,
        Database::Memory {
            capacity: 1,
            unit: Unit::MB
        }
    ));
    assert!(
        matches!(&*load("variant", "[database]\nbackend='pg'\nurl='postgres://localhost/example'")?.get::<Database>()?, Database::Postgres { url } if url == "postgres://localhost/example")
    );
    for text in [
        "[database]",
        "[database]\nbackend='missing'",
        "[database]\nbackend='memory'",
        "[database]\nbackend='memory'\ncapacity=1\nunit='mib'",
        "[database]\nbackend='postgres'\nurl='ignored'",
    ] {
        assert_eq!(
            load("variant", text).err().and_then(config_kind),
            Some(Kind::Deserialize)
        );
    }
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "http", spaces = ["expression"], default = Self { port: 8080 }, deny_unknown_fields)]
struct Expression {
    #[config(rename = "listenPort")]
    port: u16,
}

#[test]
fn type_expressions_and_field_renaming_work_without_consumer_serde() -> TestResult {
    let defaults: Expression = Default::default();
    assert_eq!(defaults.port, 8080);
    assert_eq!(load("expression", "")?.get::<Expression>()?.port, 8080);
    assert_eq!(
        load("expression", "[http]\nlistenPort=9000")?
            .get::<Expression>()?
            .port,
        9000
    );
    assert_eq!(
        load("expression", "[http]\nport=9000")
            .err()
            .and_then(config_kind),
        Some(Kind::Deserialize)
    );
    Ok(())
}

#[derive(ah_config::ConfigValue)]
#[config(default = Self { attempts: 3 })]
struct NestedExpression {
    attempts: u16,
}

#[derive(ah_config::ConfigValue)]
#[config(default = nested_factory())]
struct NestedFactory {
    delay_ms: u64,
}

fn nested_factory() -> NestedFactory {
    NestedFactory { delay_ms: 100 }
}

#[derive(ah_config::Config)]
#[config(key = "http", spaces = ["factory"], default = config_factory(), validate = validate_factory)]
struct FactoryConfig {
    port: u16,
    retry: NestedExpression,
    timing: NestedFactory,
}

fn config_factory() -> FactoryConfig {
    FactoryConfig {
        port: 0,
        retry: Default::default(),
        timing: Default::default(),
    }
}

fn validate_factory(config: &FactoryConfig) -> Result<(), &'static str> {
    if config.port == 0 {
        return Err("port must be nonzero");
    }
    Ok(())
}

#[test]
fn expressions_and_factories_generate_defaults_but_validation_belongs_to_loading() -> TestResult {
    let defaults: FactoryConfig = Default::default();
    assert_eq!(defaults.port, 0);
    assert_eq!(defaults.retry.attempts, 3);
    assert_eq!(defaults.timing.delay_ms, 100);
    assert_eq!(
        load("factory", "").err().and_then(config_kind),
        Some(Kind::Validation)
    );
    let loaded = load("factory", "[http]\nport=8080\n[http.retry]\nattempts=5")?;
    let config = loaded.get::<FactoryConfig>()?;
    assert_eq!(config.port, 8080);
    assert_eq!(config.retry.attempts, 5);
    assert_eq!(config.timing.delay_ms, 100);
    Ok(())
}

#[derive(ah_config::Config, Default)]
#[config(key = "derived", spaces = ["derived_default"])]
struct DerivedDefault {
    required: u16,
}

#[derive(ah_config::ConfigValue)]
struct ManualValue {
    value: u16,
}

impl Default for ManualValue {
    fn default() -> Self {
        Self { value: 11 }
    }
}

#[derive(ah_config::ConfigValue, Default)]
struct DerivedValue {
    value: u16,
}

#[derive(ah_config::Config)]
#[config(key = "nested", spaces = ["nested_required"])]
struct NestedRequired {
    required: NestedExpression,
    #[config(default)]
    manual: ManualValue,
    #[config(default)]
    derived: DerivedValue,
}

#[test]
fn existing_defaults_are_allowed_without_implicitly_defaulting_sections_or_fields() -> TestResult {
    assert_eq!(DerivedDefault::default().required, 0);
    assert_eq!(
        load("derived_default", "").err().and_then(config_kind),
        Some(Kind::MissingSection)
    );
    assert_eq!(
        load("derived_default", "[derived]\nrequired=7")?
            .get::<DerivedDefault>()?
            .required,
        7
    );
    assert_eq!(
        load("nested_required", "[nested]")
            .err()
            .and_then(config_kind),
        Some(Kind::Deserialize)
    );
    assert_eq!(
        load("nested_required", "[nested.required]")
            .err()
            .and_then(config_kind),
        Some(Kind::Deserialize)
    );
    let loaded = load("nested_required", "[nested.required]\nattempts=7")?;
    let config = loaded.get::<NestedRequired>()?;
    assert_eq!(config.required.attempts, 7);
    assert_eq!(config.manual.value, 11);
    assert_eq!(config.derived.value, 0);
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "generated", spaces = ["generated_default"], default)]
struct GeneratedDefault {
    #[config(default = 8080)]
    port: u16,
    #[config(default)]
    retry: Retry,
    optional: Option<String>,
}

#[test]
fn bare_type_defaults_construct_only_declared_field_defaults() -> TestResult {
    let defaults: GeneratedDefault = Default::default();
    assert_eq!(defaults.port, 8080);
    assert_eq!(defaults.retry.attempts, 3);
    assert!(defaults.optional.is_none());
    let loaded = load("generated_default", "")?;
    let config = loaded.get::<GeneratedDefault>()?;
    assert_eq!(config.port, defaults.port);
    assert_eq!(config.retry.attempts, defaults.retry.attempts);
    assert_eq!(config.retry.delay_ms, defaults.retry.delay_ms);
    assert_eq!(config.optional, defaults.optional);
    Ok(())
}

#[derive(ah_config::Config)]
#[config(key = "storage", spaces = ["enum_factory"], tag = "backend", default = storage_factory())]
enum FactoryStorage {
    Local { path: String },
    Memory { capacity: u64 },
}

fn storage_factory() -> FactoryStorage {
    FactoryStorage::Local {
        path: "data".into(),
    }
}

#[test]
fn enum_factories_generate_default_without_weakening_explicit_variant_requirements() -> TestResult {
    assert!(matches!(FactoryStorage::default(), FactoryStorage::Local { path } if path == "data"));
    let loaded = load("enum_factory", "")?;
    assert!(
        matches!(&*loaded.get::<FactoryStorage>()?, FactoryStorage::Local { path } if path == "data")
    );
    assert_eq!(
        load("enum_factory", "[storage]\nbackend='local'")
            .err()
            .and_then(config_kind),
        Some(Kind::Deserialize)
    );
    let loaded = load("enum_factory", "[storage]\nbackend='memory'\ncapacity=512")?;
    assert!(matches!(
        &*loaded.get::<FactoryStorage>()?,
        FactoryStorage::Memory { capacity: 512 }
    ));
    Ok(())
}
