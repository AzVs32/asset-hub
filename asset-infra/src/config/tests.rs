use super::*;

#[test]
fn empty_config_uses_defaults() {
    let config = AssetInfraConfig::from_config_str("").unwrap();

    assert_eq!(
        config.database.sqlite_path,
        PathBuf::from(DEFAULT_SQLITE_PATH)
    );
    assert_eq!(
        config.database.max_connections,
        DEFAULT_SQLITE_MAX_CONNECTIONS
    );
    assert_eq!(config.blob.fs_root, PathBuf::from(DEFAULT_FS_ROOT));
    assert!(config.kind.plugin_manifests.is_empty());
}

#[test]
fn partial_config_keeps_missing_defaults() {
    let config = AssetInfraConfig::from_config_str(
        r#"
        [blob]
        fs_root = "tmp/blob"
        "#,
    )
    .unwrap();

    assert_eq!(
        config.database.sqlite_path,
        PathBuf::from(DEFAULT_SQLITE_PATH)
    );
    assert_eq!(config.blob.fs_root, PathBuf::from("tmp/blob"));
}

#[test]
fn kind_config_accepts_static_definitions_and_plugin_manifests() {
    let config = AssetInfraConfig::from_config_str(
        r#"
        [kind]
        plugin_manifests = ["plugins/example.json"]

        [[kind.definitions]]
        kind = "doc:note"
        label = "Note"
        supports_content = false
        "#,
    )
    .unwrap();

    assert_eq!(
        config.kind.plugin_manifests,
        [PathBuf::from("plugins/example.json")]
    );
    assert_eq!(config.kind.definitions.len(), 1);
    assert_eq!(config.kind.definitions[0].kind, "doc:note");
    assert_eq!(config.kind.definitions[0].label.as_deref(), Some("Note"));
    assert!(!config.kind.definitions[0].supports_content);
}

#[test]
fn optional_missing_config_file_uses_defaults() {
    let config = AssetInfraConfig::from_optional_config_file(
        std::env::temp_dir().join("asset-hub-missing-config.toml"),
    )
    .unwrap();

    assert_eq!(
        config.database.sqlite_path,
        PathBuf::from(DEFAULT_SQLITE_PATH)
    );
    assert_eq!(config.blob.fs_root, PathBuf::from(DEFAULT_FS_ROOT));
}

#[test]
fn normalized_config_turns_relative_paths_into_absolute_paths() {
    let config = AssetInfraConfig::default().normalized().unwrap();

    assert!(config.database.sqlite_path.is_absolute());
    assert!(config.blob.fs_root.is_absolute());
    assert!(config.kind.plugin_manifests.is_empty());
}

#[test]
fn normalized_config_turns_plugin_manifests_into_absolute_paths() {
    let config = AssetInfraConfig {
        kind: KindRegistryConfig {
            plugin_manifests: vec![PathBuf::from("plugins/example.json")],
            ..KindRegistryConfig::default()
        },
        ..AssetInfraConfig::default()
    }
    .normalized()
    .unwrap();

    assert!(config.kind.plugin_manifests[0].is_absolute());
}

#[test]
fn plugin_host_policy_parses_budgets_and_normalizes_grants() {
    let config = AssetInfraConfig::from_config_str(
        r#"
        [plugin]
        max_content_bytes = 1024
        max_inline_content_bytes = 512
        max_content_read_bytes = 256
        max_input_bytes = 2048
        max_output_bytes = 4096
        max_concurrent_calls = 2
        memory_max_pages = 128
        timeout_seconds = 5

        [plugin.grants]
        network_hosts = ["api.example.com"]
        filesystem_read = ["plugin-data"]
        filesystem_write = []
        "#,
    )
    .unwrap()
    .normalized()
    .unwrap();

    assert_eq!(config.plugin.max_concurrent_calls, 2);
    assert_eq!(
        config
            .plugin
            .execution_policy()
            .unwrap()
            .max_content_bytes(),
        1024
    );
    assert_eq!(config.plugin.grants.network_hosts, ["api.example.com"]);
    assert!(config.plugin.grants.filesystem_read[0].is_absolute());
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
fn configured_content_limit_is_the_runtime_policy_limit() {
    let config = AssetInfraConfig::from_config_str(
        r#"
        [plugin]
        max_content_bytes = 134217728
        "#,
    )
    .unwrap()
    .normalized()
    .unwrap();

    let policy = config.plugin.execution_policy().unwrap();
    assert_eq!(policy.max_content_bytes(), 128 * 1024 * 1024);
    assert_eq!(
        policy.max_inline_content_bytes(),
        DEFAULT_PLUGIN_MAX_INLINE_CONTENT_BYTES
    );
}
