use super::*;

#[test]
fn parses_each_top_level_command_group() {
    for (args, expected) in [
        (vec!["asset", "system", "--scan-resource"], "system"),
        (vec!["asset", "user", "--list"], "user"),
        (vec!["asset", "plugin"], "plugin"),
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
fn parses_config_check_and_show_with_optional_paths() {
    for args in [
        vec!["asset", "config", "--check"],
        vec!["asset", "config", "--check", "custom.toml"],
        vec!["asset", "config", "--show"],
        vec!["asset", "config", "--show", "custom.toml"],
    ] {
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Command::Config(_)
        ));
    }
}

#[test]
fn config_requires_exactly_one_operation() {
    assert!(Cli::try_parse_from(["asset", "config"]).is_err());
    assert!(Cli::try_parse_from(["asset", "config", "--check", "--show"]).is_err());
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
fn user_requires_exactly_one_operation() {
    assert!(Cli::try_parse_from(["asset", "user"]).is_err());
    assert!(Cli::try_parse_from(["asset", "user", "--list", "--show", "alice"]).is_err());
}
