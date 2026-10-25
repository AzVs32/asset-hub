//! Apply configuration changes without altering values already in use.
//!
//! Read the core examples first. This lifecycle example demonstrates explicit
//! reloads and ownership; it does not implement automatic file watching.
//!
//! Repeated get calls share an Arc within a snapshot. Each load creates distinct
//! values, even for the same space and file. Editing the file does not update
//! old values, and retrieved handles can outlive their LoadedConfig container.
//!
//! Run from the ah-config source root: `cargo run --example snapshots`.

use ah_config::Registry;
use std::sync::Arc;

#[derive(ah_config::Config)]
#[config(key = "logging")]
struct LoggingConfig {
    level: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    // Update a temporary file using embedded fixtures so the originals stay unchanged.
    std::fs::write(&path, include_str!("snapshots_initial.toml"))?;

    let first = Registry::for_space("default")?.load_required(&path)?;
    let original = first.get::<LoggingConfig>()?;
    assert!(Arc::ptr_eq(&original, &first.get::<LoggingConfig>()?));

    let second = Registry::for_space("default")?.load_required(&path)?;
    assert_eq!(second.get::<LoggingConfig>()?.level, original.level);
    assert!(!Arc::ptr_eq(&original, &second.get::<LoggingConfig>()?));

    std::fs::write(&path, include_str!("snapshots_updated.toml"))?;
    assert_eq!(first.get::<LoggingConfig>()?.level, "info");
    let updated = Registry::for_space("default")?.load_required(&path)?;
    assert_eq!(updated.get::<LoggingConfig>()?.level, "debug");
    assert_eq!(original.level, "info");
    assert!(!Arc::ptr_eq(&original, &updated.get::<LoggingConfig>()?));

    drop(first);
    assert_eq!(original.level, "info");
    println!("Loaded updated logging settings while retaining the original snapshot value.");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_snapshot_values_independent_and_shared() -> Result<(), Box<dyn std::error::Error>> {
        super::main()
    }
}
