use super::*;
use clap::CommandFactory;

#[test]
fn http_cli_arguments_have_no_environment_variable_sources() {
    let help = HttpCli::command().render_help().to_string();
    assert!(!help.contains("[env:"));
}

#[test]
fn clap_accepts_only_canonical_boolean_values() {
    assert!(HttpCli::try_parse_from(["asset-http", "--enable-swagger=true"]).is_ok());
    assert!(HttpCli::try_parse_from(["asset-http", "--enable-swagger=false"]).is_ok());
    assert!(HttpCli::try_parse_from(["asset-http", "--enable-swagger=yes"]).is_err());
    assert!(HttpCli::try_parse_from(["asset-http", "--enable-swagger=1"]).is_err());
}

#[test]
fn clap_rejects_invalid_http_boundaries() {
    assert!(HttpCli::try_parse_from(["asset-http", "--session-inactivity-secs", "0"]).is_err());
    assert!(HttpCli::try_parse_from(["asset-http", "--cors-allowed-origins", "*"]).is_err());
}

#[test]
fn parses_cors_policy() {
    assert_eq!(parse_cors_policy("").unwrap(), CorsPolicy::None);
    assert!(parse_cors_policy("*").is_err());
    assert_eq!(
        parse_cors_policy("http://127.0.0.1:5173, https://example.com").unwrap(),
        CorsPolicy::Origins(vec![
            HeaderValue::from_static("http://127.0.0.1:5173"),
            HeaderValue::from_static("https://example.com"),
        ])
    );
}
