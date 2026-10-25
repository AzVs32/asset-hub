//! Declare a section, select a space, load a file, then get its typed configuration.
//!
//! Config registers HttpConfig at service.http. ConfigValue describes TlsConfig
//! as a nested value, accessed through http.tls rather than a separate section.
//! Rust fields and TOML keys share snake_case names.
//!
//! All fields except public_url are required; an omitted Option becomes None.
//! validate_http checks the assembled section, including its nested TLS values.
//! Required fields must exist; validation additionally rejects unusable values.
//! Certificate paths are values only; loading does not open certificate files.
//!
//! Read basic.toml alongside this file.
//! Run from the ah-config source root: `cargo run --example basic`.

use ah_config::{ConfigErrorKind, Registry};
use std::path::{Path, PathBuf};

#[derive(ah_config::Config)]
#[config(key = "service.http", validate = validate_http)]
struct HttpConfig {
    host: String,
    port: u16,
    public_url: Option<String>,
    tls: TlsConfig,
}

#[derive(ah_config::ConfigValue)]
struct TlsConfig {
    certificate_file: PathBuf,
    private_key_file: PathBuf,
}

fn validate_http(config: &HttpConfig) -> Result<(), &'static str> {
    if config.host.trim().is_empty() {
        return Err("HTTP host cannot be empty");
    }
    if config.port == 0 {
        return Err("HTTP port must be nonzero");
    }
    if config.tls.certificate_file.as_os_str().is_empty()
        || config.tls.private_key_file.as_os_str().is_empty()
    {
        return Err("TLS certificate and private key paths cannot be empty");
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Registry::for_space("default")?.load_required("examples/basic.toml")?;
    let http = config.get::<HttpConfig>()?;
    assert_eq!(http.host, "localhost");
    assert_eq!(http.port, 8443);
    assert!(http.public_url.is_none());
    assert_eq!(http.tls.certificate_file, Path::new("server.crt"));
    assert_eq!(http.tls.private_key_file, Path::new("server.key"));

    // ConfigValue does not register an independently retrievable section.
    assert_eq!(
        config.get::<TlsConfig>().err().map(|error| error.kind()),
        Some(ConfigErrorKind::NotRegistered)
    );
    println!(
        "HTTPS endpoint: {}:{}, certificate: {}",
        http.host,
        http.port,
        http.tls.certificate_file.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn loads_a_section_with_nested_values() -> Result<(), Box<dyn std::error::Error>> {
        super::main()
    }

    #[test]
    fn rejects_an_unusable_http_port() -> Result<(), Box<dyn std::error::Error>> {
        let error = super::Registry::for_space("default")?
            .load_required("examples/basic_invalid.toml")
            .err()
            .ok_or("expected HTTP validation to fail")?;
        assert_eq!(error.kind(), super::ConfigErrorKind::Validation);
        Ok(())
    }
}
