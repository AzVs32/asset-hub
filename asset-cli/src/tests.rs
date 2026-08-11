use super::*;
use clap::{CommandFactory, error::ErrorKind};

#[test]
fn plugin_help_describes_plugin_id_operation_options() {
    let help = Cli::command()
        .find_subcommand_mut("plugin")
        .unwrap()
        .render_help()
        .to_string();

    assert!(help.contains("--seal <PLUGIN_ID>"));
    assert!(help.contains("--verify <PLUGIN_ID>"));
    assert!(!help.contains("PACKAGE"));
}

#[test]
fn plugin_requires_exactly_one_operation() {
    let missing = Cli::try_parse_from(["asset", "plugin"]).unwrap_err();
    assert_eq!(missing.kind(), ErrorKind::MissingRequiredArgument);

    let conflicting = Cli::try_parse_from([
        "asset",
        "plugin",
        "--seal",
        "example.plugin",
        "--verify",
        "example.plugin",
    ])
    .unwrap_err();
    assert_eq!(conflicting.kind(), ErrorKind::ArgumentConflict);
}

#[tokio::test]
async fn plugin_id_cannot_escape_the_configured_packages_directory() {
    let cli = Cli::try_parse_from(["asset", "plugin", "--verify", "../example.plugin"]).unwrap();
    let error = run(cli).await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "plugin id must be a single package directory name: `../example.plugin`"
    );
}
