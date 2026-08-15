use super::*;

#[tokio::test]
async fn plugin_id_cannot_escape_the_configured_packages_directory() {
    let cli = Cli::try_parse_from(["asset", "plugin", "--verify", "../example.plugin"]).unwrap();
    let error = run(cli).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "plugin id must be a single package directory name: `../example.plugin`"
    );
}
