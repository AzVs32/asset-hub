use super::*;

#[test]
fn parses_each_top_level_command_group() {
    for (name, expected) in [("system", "system"), ("user", "user"), ("plugin", "plugin")] {
        let cli = Cli::try_parse_from(["asset", name]).unwrap();
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
fn rejects_unknown_command_groups() {
    assert!(Cli::try_parse_from(["asset", "unknown"]).is_err());
}
