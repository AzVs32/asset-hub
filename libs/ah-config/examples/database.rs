//! Select a configuration shape with an enum, rather than making every field optional.
//!
//! Read after defaults.rs. key selects the database section; tag selects a variant
//! inside that section. Each variant then requires only its own fields. A pure
//! unit enum such as CapacityUnit is a string value and does not need a tag.
//!
//! Self::Sqlite supplies a missing section and derives its Default from field
//! declarations. An existing section still needs backend. Selecting Postgres
//! requires url; it cannot fall back to SQLite. The Memory fixtures contrast an
//! explicit MB unit with an omitted unit that defaults to MiB.
//! validate_database checks only the selected variant: nonempty paths and URLs,
//! or a nonzero memory capacity. It does not open files or check connectivity.
//!
//! Read the database_*.toml files alongside this file. This example loads values;
//! it does not convert capacities, allocate memory or connect to a database.
//! Run from the ah-config source root: `cargo run --example database`.

use ah_config::Registry;
use std::path::PathBuf;

#[derive(ah_config::Config)]
#[config(
    key = "database",
    tag = "backend",
    default = Self::Sqlite,
    validate = validate_database,
)]
enum DatabaseConfig {
    Sqlite {
        #[config(default = "example.sqlite".into())]
        path: PathBuf,
    },
    Memory {
        capacity: u64,
        #[config(default)]
        unit: CapacityUnit,
    },
    Postgres {
        url: String,
    },
}

#[derive(ah_config::ConfigValue)]
#[config(default = Self::MiB)]
enum CapacityUnit {
    #[config(rename = "B")]
    B,
    #[config(rename = "KB")]
    KB,
    #[config(rename = "MB")]
    MB,
    #[config(rename = "GB")]
    GB,
    #[config(rename = "KiB")]
    KiB,
    #[config(rename = "MiB")]
    MiB,
    #[config(rename = "GiB")]
    GiB,
}

fn validate_database(config: &DatabaseConfig) -> Result<(), &'static str> {
    match config {
        DatabaseConfig::Sqlite { path } if path.as_os_str().is_empty() => {
            Err("SQLite path cannot be empty")
        }
        DatabaseConfig::Memory { capacity: 0, .. } => Err("memory capacity must be nonzero"),
        DatabaseConfig::Postgres { url } if url.trim().is_empty() => {
            Err("Postgres URL cannot be empty")
        }
        _ => Ok(()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let defaults =
        Registry::for_space("default")?.load_required("examples/database_defaults.toml")?;
    assert!(matches!(
        &*defaults.get::<DatabaseConfig>()?,
        DatabaseConfig::Sqlite { path } if path == &PathBuf::from("example.sqlite")
    ));

    let memory = Registry::for_space("default")?.load_required("examples/database_memory.toml")?;
    assert!(matches!(
        &*memory.get::<DatabaseConfig>()?,
        DatabaseConfig::Memory {
            capacity: 512,
            unit: CapacityUnit::MB
        }
    ));

    let implicit_unit = Registry::for_space("default")?
        .load_required("examples/database_memory_default_unit.toml")?;
    assert!(matches!(
        &*implicit_unit.get::<DatabaseConfig>()?,
        DatabaseConfig::Memory {
            capacity: 512,
            unit: CapacityUnit::MiB
        }
    ));

    let postgres =
        Registry::for_space("default")?.load_required("examples/database_postgres.toml")?;
    assert!(matches!(
        &*postgres.get::<DatabaseConfig>()?,
        DatabaseConfig::Postgres { url } if url == "postgresql://app:example@localhost:5432/example"
    ));
    println!("Absent section: SQLite; selected variants: Memory with MB/MiB, and Postgres.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn selects_a_shape_and_applies_its_field_defaults() -> Result<(), Box<dyn std::error::Error>> {
        super::main()
    }

    #[test]
    fn invalid_selected_variants_do_not_restore_sqlite() -> Result<(), Box<dyn std::error::Error>> {
        let error = super::Registry::for_space("default")?
            .load_required("examples/database_invalid_memory.toml")
            .err()
            .ok_or("expected memory capacity validation to fail")?;
        assert_eq!(error.kind(), ah_config::ConfigErrorKind::Validation);
        Ok(())
    }
}
