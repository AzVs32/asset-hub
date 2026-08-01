use super::*;

#[test]
fn parses_each_top_level_command_group() {
    for (args, expected) in [
        (vec!["asset", "system", "--scan-resource"], "system"),
        (vec!["asset", "user", "--list"], "user"),
        (vec!["asset", "plugin", "--verify", "plugin.json"], "plugin"),
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        let actual = match cli.command {
            Command::Config(_) => "config",
            Command::System(_) => "system",
            Command::User(_) => "user",
            Command::Plugin(_) => "plugin",
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn parses_plugin_verify_path() {
    assert!(matches!(
        Cli::try_parse_from(["asset", "plugin", "--verify", "path/plugin.json"])
            .unwrap()
            .command,
        Command::Plugin(_)
    ));
}

#[test]
fn parses_plugin_generate_lock_path() {
    assert!(matches!(
        Cli::try_parse_from(["asset", "plugin", "--generate-lock", "path/manifest.json"])
            .unwrap()
            .command,
        Command::Plugin(_)
    ));
}

#[test]
fn plugin_requires_exactly_one_operation() {
    assert!(Cli::try_parse_from(["asset", "plugin"]).is_err());
    assert!(Cli::try_parse_from(["asset", "plugin", "--seal", "plugin.json"]).is_err());
    assert!(
        Cli::try_parse_from([
            "asset",
            "plugin",
            "--verify",
            "plugin.json",
            "--generate-lock",
            "plugin.json"
        ])
        .is_err()
    );
}

#[test]
fn rejects_removed_plugin_operations() {
    for args in [
        vec!["asset", "plugin", "gen"],
        vec!["asset", "plugin", "--verify-wasm", "plugin.json"],
        vec!["asset", "plugin", "--verify-web", "plugin.json"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}

#[test]
fn parses_config_check_and_show_with_global_path() {
    for args in [
        vec!["asset", "config", "--check"],
        vec!["asset", "--config", "custom.toml", "config", "--check"],
        vec!["asset", "config", "--show"],
        vec!["asset", "--config", "custom.toml", "config", "--show"],
    ] {
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Command::Config(_)
        ));
    }
}

#[test]
fn parses_one_global_config_path() {
    let cli = Cli::try_parse_from(["asset", "--config", "custom.toml", "user", "--list"]).unwrap();
    assert_eq!(cli.config, Some("custom.toml".into()));
}

#[test]
fn config_requires_exactly_one_operation() {
    assert!(Cli::try_parse_from(["asset", "config"]).is_err());
    assert!(Cli::try_parse_from(["asset", "config", "--check", "--show"]).is_err());
}

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

#[test]
fn system_requires_an_operation() {
    assert!(Cli::try_parse_from(["asset", "system"]).is_err());
    assert!(matches!(
        Cli::try_parse_from(["asset", "system", "--scan-resource"])
            .unwrap()
            .command,
        Command::System(_)
    ));
}

#[test]
fn rejects_unknown_command_groups() {
    assert!(Cli::try_parse_from(["asset", "unknown"]).is_err());
}

#[test]
fn parses_all_user_operations() {
    for args in [
        vec!["asset", "user", "--list"],
        vec!["asset", "user", "--create", "alice"],
        vec!["asset", "user", "--create", "admin", "--admin"],
        vec!["asset", "user", "--password", "alice"],
        vec!["asset", "user", "--enable", "alice"],
        vec!["asset", "user", "--disable", "alice"],
        vec!["asset", "user", "--show", "alice"],
    ] {
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Command::User(_)
        ));
    }
}

#[test]
fn user_admin_flag_requires_create() {
    assert!(Cli::try_parse_from(["asset", "user", "--admin"]).is_err());
    assert!(Cli::try_parse_from(["asset", "user", "--list", "--admin"]).is_err());
}

#[test]
fn user_requires_exactly_one_operation() {
    assert!(Cli::try_parse_from(["asset", "user"]).is_err());
    assert!(Cli::try_parse_from(["asset", "user", "--list", "--show", "alice"]).is_err());
}
