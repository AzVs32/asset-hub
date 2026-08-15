use super::*;

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
