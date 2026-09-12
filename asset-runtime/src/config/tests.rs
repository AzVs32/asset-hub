use super::*;
use asset_config::{ConfigError, ConfigRegistry, ConfigSection};

fn load(source: &str) -> Result<AssetConfig, ConfigError> {
    ConfigRegistry::new()
        .with::<AssetConfig>()?
        .load_str(source)?
        .section::<AssetConfig>()
        .cloned()
}

#[test]
fn normalized_config_turns_relative_paths_into_absolute_paths() {
    let config = AssetConfig::default().normalize().unwrap();

    assert!(config.blob.local.root.is_absolute());
}

#[test]
fn asset_section_strictly_owns_its_subtree() {
    for source in [
        "[asset.database]\nsqlite_path = \"custom.sqlite\"",
        "[asset.database.sqlite]\npath = \"custom.sqlite\"",
        "[asset]\nunknown_section = true",
        "[asset.unsupported]\nmax_connections = 8",
    ] {
        assert!(load(source).is_err());
    }
}

#[test]
fn asset_section_enforces_non_obvious_runtime_constraints() {
    let unsupported = load("[asset.database]\nbackend = \"postgresql\"").unwrap_err();
    assert!(unsupported.to_string().contains("postgresql"));

    let zero_edit_limit = load("[asset.resource_edit]\nmax_text_bytes = 0").unwrap_err();
    assert!(zero_edit_limit.to_string().contains("greater than zero"));

    for seconds in [0, 86_401, i64::MAX as u64] {
        let error = load(&format!(
            "[asset.idempotency]\nlease_duration_seconds = {seconds}"
        ))
        .unwrap_err();
        assert!(error.to_string().contains(&format!(
            "idempotency.lease_duration_seconds must be between 1 and 86400 seconds; got {seconds}"
        )));
    }
}
