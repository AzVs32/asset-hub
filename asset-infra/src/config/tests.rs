use super::*;

#[test]
fn normalized_config_turns_relative_paths_into_absolute_paths() {
    let config = AssetInfraConfig::default().normalized().unwrap();

    assert!(config.blob.local.root.is_absolute());
    assert!(config.sqlite_path().is_absolute());
    assert_eq!(
        config.sqlite_path(),
        config.blob.local.root.join(SQLITE_DATABASE_RELATIVE_PATH)
    );
    assert_eq!(
        config.plugin_packages_path(),
        config.blob.local.root.join(PLUGIN_PACKAGES_RELATIVE_PATH)
    );
}

#[test]
fn config_rejects_manually_configured_sqlite_path() {
    for source in [
        "[database]\nsqlite_path = \"custom.sqlite\"",
        "[database.sqlite]\npath = \"custom.sqlite\"",
    ] {
        let error = AssetInfraConfig::from_config_str(source).unwrap_err();
        assert!(error.to_string().contains("path"));
    }
}

#[test]
fn config_rejects_unknown_top_level_and_kind_fields() {
    for source in [
        "unknown_section = true",
        "[kind]\nplugin_manifests = [\"plugin.json\"]",
        "[kind]\nplugin_manifest = [\"plugin.json\"]",
        "[[kind.definitions]]\nkind = \"doc:note\"",
    ] {
        assert!(AssetInfraConfig::from_config_str(source).is_err());
    }
}

#[test]
fn config_rejects_unsupported_backends() {
    let database_error = AssetInfraConfig::from_config_str(
        r#"
        [database]
        backend = "postgresql"
        "#,
    )
    .unwrap_err();
    assert!(database_error.to_string().contains("postgresql"));

    let blob_error = AssetInfraConfig::from_config_str(
        r#"
        [blob]
        backend = "s3"
        "#,
    )
    .unwrap_err();
    assert!(blob_error.to_string().contains("s3"));
}

#[test]
fn plugin_host_policy_rejects_unbounded_or_zero_values() {
    let wildcard = AssetInfraConfig::from_config_str(
        r#"
        [plugin.grants]
        network_hosts = ["*"]
        "#,
    )
    .unwrap()
    .normalized();
    assert!(wildcard.is_err());

    let zero = AssetInfraConfig::from_config_str(
        r#"
        [plugin]
        max_concurrent_calls = 0
        "#,
    )
    .unwrap()
    .normalized();
    assert!(zero.is_err());
}

#[test]
fn resource_edit_policy_is_independent_and_rejects_zero() {
    let config = AssetInfraConfig::from_config_str(
        r#"
        [resource_edit]
        max_text_bytes = 2097152
        "#,
    )
    .unwrap()
    .normalized()
    .unwrap();
    assert_eq!(config.resource_edit.max_text_bytes, 2 * 1024 * 1024);

    let invalid = AssetInfraConfig::from_config_str(
        r#"
        [resource_edit]
        max_text_bytes = 0
        "#,
    )
    .unwrap()
    .normalized();
    assert!(invalid.is_err());
}

#[test]
fn idempotency_lease_duration_uses_the_supported_range() {
    for seconds in [1, 300, 86_400] {
        let config = AssetInfraConfig::from_config_str(&format!(
            "[idempotency]\nlease_duration_seconds = {seconds}"
        ))
        .unwrap()
        .normalized()
        .unwrap();
        assert_eq!(config.idempotency.lease_duration().as_secs(), seconds);
    }

    let default = AssetInfraConfig::from_config_str("")
        .unwrap()
        .normalized()
        .unwrap();
    assert_eq!(default.idempotency.lease_duration().as_secs(), 300);

    for seconds in [0, 86_401, i64::MAX as u64] {
        let error = AssetInfraConfig::from_config_str(&format!(
            "[idempotency]\nlease_duration_seconds = {seconds}"
        ))
        .unwrap()
        .normalized()
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "invalid configuration: idempotency.lease_duration_seconds must be between 1 and 86400 seconds; got {seconds}"
            )
        );
    }

    let overflow = AssetInfraConfig::from_config_str(&format!(
        "[idempotency]\nlease_duration_seconds = {}",
        u64::MAX
    ))
    .unwrap_err();
    assert!(overflow.to_string().contains("invalid configuration"));
    assert!(overflow.to_string().contains("u64 value was too large"));

    let error = IdempotencyConfig {
        lease_duration_seconds: u64::MAX,
    }
    .validate()
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid configuration: idempotency.lease_duration_seconds must be between 1 and 86400 seconds; got 18446744073709551615"
    );
}
