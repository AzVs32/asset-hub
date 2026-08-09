use super::*;

#[tokio::test]
async fn config_path_is_rejected_for_plugin_commands() {
    let cli = Cli::try_parse_from([
        "asset",
        "--config",
        "custom.toml",
        "plugin",
        "--verify",
        "plugin.json",
    ])
    .unwrap();
    let error = run(cli).await.unwrap_err();
    assert_eq!(error.to_string(), "--config is not used by `asset plugin`");
}
