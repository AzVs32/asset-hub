use super::*;
use clap::{CommandFactory, error::ErrorKind};

#[tokio::test]
async fn config_path_is_rejected_for_plugin_commands() {
    let cli = Cli::try_parse_from([
        "asset",
        "--config",
        "custom.toml",
        "plugin",
        "--verify",
        "example.plugin",
    ])
    .unwrap();
    let error = run(cli).await.unwrap_err();
    assert_eq!(error.to_string(), "--config is not used by `asset plugin`");
}

#[test]
fn plugin_help_describes_package_operation_options() {
    let help = Cli::command()
        .find_subcommand_mut("plugin")
        .unwrap()
        .render_help()
        .to_string();

    assert!(help.contains("--seal <PACKAGE>"));
    assert!(help.contains("--verify <PACKAGE>"));
    assert!(!help.contains("generate-lock"));
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
