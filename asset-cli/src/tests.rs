use super::*;

#[tokio::test]
async fn plugin_id_cannot_escape_the_configured_packages_directory() {
    let cli = Cli::try_parse_from(["asset", "plugin", "--uninstall", "../example.plugin"]).unwrap();
    let error = run(cli).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "invalid configuration: plugin id must be a single package directory name: `../example.plugin`"
    );
}

#[tokio::test]
async fn remote_plugin_install_sources_are_explicitly_unsupported() {
    let cli = Cli::try_parse_from([
        "asset",
        "plugin",
        "--install",
        "https://github.com/example/plugin",
    ])
    .unwrap();
    let error = run(cli).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "remote plugin sources are not supported yet; --install expects a local directory path"
    );
}
