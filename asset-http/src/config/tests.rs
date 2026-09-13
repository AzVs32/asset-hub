use super::*;
use asset_config::ConfigRegistry;

#[test]
fn registered_http_config_owns_transport_settings() {
    let registry = ConfigRegistry::new().with::<HttpConfig>().unwrap();
    let loaded = registry
        .load_str(
            r#"
            [http]
            addr = "0.0.0.0:9000"
            cors_allowed_origins = ["http://127.0.0.1:5173", "https://example.com"]
            request_timeout_seconds = 45

            [http.archive]
            max_concurrent = 3
            max_bytes = 1024
            "#,
        )
        .unwrap();
    let config = loaded.section::<HttpConfig>().unwrap();
    let options = config.clone().into_runtime_options().unwrap();

    assert_eq!(options.addr, "0.0.0.0:9000".parse().unwrap());
    assert_eq!(options.archive.max_concurrent.get(), 3);
    assert_eq!(options.archive.max_bytes.get(), 1024);
    assert_eq!(
        options.router.cors,
        CorsPolicy::Origins(vec![
            HeaderValue::from_static("http://127.0.0.1:5173"),
            HeaderValue::from_static("https://example.com"),
        ])
    );
    assert_eq!(options.router.request_timeout.as_secs(), 45);
}

#[test]
fn registered_http_config_rejects_invalid_owned_values() {
    let registry = ConfigRegistry::new().with::<HttpConfig>().unwrap();

    assert!(
        registry
            .load_str("[http]\nrequest_timeout_seconds = 0")
            .is_err()
    );
    assert!(
        registry
            .load_str("[http]\ncors_allowed_origins = [\"*\"]")
            .is_err()
    );
}
