//! Decide which missing fields can be supplied by the configuration declaration.
//!
//! Read after basic.rs. The default attribute accepts an expression, including a
//! function call, or no value to use the field type's Default implementation.
//! Option fields need no attribute to become None when absent. An ordinary field
//! without a default remains required, even if its type implements Default.
//!
//! All fields here can be supplied without a file, so load_optional succeeds for
//! a missing path. load_required would still require that file to exist.
//! defaults_partial.toml then overrides the timeout, one nested field and the mirror
//! list. These defaults fill missing input; they never replace invalid input.
//! validate_client checks both defaulted and supplied values. An explicit zero
//! timeout fails validation instead of restoring the default timeout.
//!
//! Run from the ah-config source root: `cargo run --example defaults`.

use ah_config::Registry;

#[derive(ah_config::Config)]
#[config(key = "client", validate = validate_client)]
struct ClientConfig {
    #[config(default = "https://primary.example.com".into())]
    endpoint: String,
    #[config(default = 5_000)]
    timeout_ms: u64,
    #[config(default = default_mirrors())]
    mirrors: Vec<String>,
    #[config(default)]
    retry: RetryConfig,
    token: Option<String>,
}

// The type-level marker generates Default; the parent field opts into using it.
#[derive(ah_config::ConfigValue)]
#[config(default)]
struct RetryConfig {
    #[config(default = 3)]
    attempts: u16,
    #[config(default = 100)]
    delay_ms: u64,
}

fn default_mirrors() -> Vec<String> {
    vec!["https://fallback.example.com".into()]
}

fn validate_client(config: &ClientConfig) -> Result<(), &'static str> {
    if config.endpoint.trim().is_empty() {
        return Err("client endpoint cannot be empty");
    }
    if config.timeout_ms == 0 {
        return Err("client timeout must be nonzero");
    }
    if config.retry.attempts == 0 {
        return Err("retry attempts must be nonzero");
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let defaults = Registry::for_space("default")?
        .with_base_dir(directory.path())
        .load_optional("missing.toml")?;
    let client = defaults.get::<ClientConfig>()?;
    assert_eq!(client.endpoint, "https://primary.example.com");
    assert_eq!(client.timeout_ms, 5_000);
    assert_eq!(client.mirrors, ["https://fallback.example.com"]);
    assert_eq!(client.retry.attempts, 3);
    assert_eq!(client.retry.delay_ms, 100);
    assert!(client.token.is_none());

    let configured =
        Registry::for_space("default")?.load_required("examples/defaults_partial.toml")?;
    let client = configured.get::<ClientConfig>()?;
    assert_eq!(client.endpoint, "https://primary.example.com");
    assert_eq!(client.timeout_ms, 1_000);
    assert!(client.mirrors.is_empty());
    assert_eq!(client.retry.attempts, 5);
    assert_eq!(client.retry.delay_ms, 100);
    assert!(client.token.is_none());
    println!(
        "Endpoint: {}, configured timeout: {} ms, fallback mirrors removed: {}",
        client.endpoint,
        client.timeout_ms,
        client.mirrors.is_empty()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn fills_missing_fields_and_keeps_explicit_values() -> Result<(), Box<dyn std::error::Error>> {
        super::main()
    }

    #[test]
    fn invalid_values_never_fall_back_to_defaults() -> Result<(), Box<dyn std::error::Error>> {
        use ah_config::ConfigErrorKind;

        for (path, kind) in [
            (
                "examples/defaults_invalid_timeout.toml",
                ConfigErrorKind::Validation,
            ),
            (
                "examples/defaults_invalid_type.toml",
                ConfigErrorKind::Deserialize,
            ),
        ] {
            let error = super::Registry::for_space("default")?
                .load_required(path)
                .err()
                .ok_or("expected invalid client configuration to fail")?;
            assert_eq!(error.kind(), kind);
        }
        Ok(())
    }
}
