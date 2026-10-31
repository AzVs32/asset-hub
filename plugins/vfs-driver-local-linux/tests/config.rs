#![cfg(target_os = "linux")]
use ah_config::{ConfigErrorKind, ConfigSection, Registry};
use ah_vfs::config::{DriverConfigNamespace, VfsConfig};
use std::{error::Error, path::Path};
use vfs_driver_local_linux::LinuxLocalDriverConfig;

type TestResult = Result<(), Box<dyn Error>>;

#[derive(ah_config::Config)]
#[config(parent = DriverConfigNamespace, key = "external_test", default)]
struct ExternalDriverConfig {
    #[config(default = 5)]
    workers: u16,
}

#[test]
fn local_and_external_driver_sections_extend_the_vfs_namespace_independently() -> TestResult {
    assert_eq!(VfsConfig::KEY, "vfs");
    assert_eq!(DriverConfigNamespace::KEY, "vfs.driver");
    assert_eq!(LinuxLocalDriverConfig::KEY, "vfs.driver.local");
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[vfs.driver.local]\nbackend = 'memory'\n[vfs.driver.external_test]\nworkers = 9\n",
    )?;
    let loaded = Registry::for_space("default")?.load_required(&path)?;
    loaded.get::<VfsConfig>()?;
    loaded.get::<DriverConfigNamespace>()?;
    assert!(matches!(
        &*loaded.get::<LinuxLocalDriverConfig>()?,
        LinuxLocalDriverConfig::Memory {}
    ));
    assert_eq!(loaded.get::<ExternalDriverConfig>()?.workers, 9);
    Ok(())
}

#[test]
fn omitted_local_settings_default_to_sqlite_without_creating_files() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("missing/config.toml");
    let loaded = Registry::for_space("default")?.load_optional(&path)?;
    assert!(matches!(
        &*loaded.get::<LinuxLocalDriverConfig>()?,
        LinuxLocalDriverConfig::Sqlite { path } if path == Path::new("vfs-driver-local.sqlite")
    ));
    assert!(matches!(
        LinuxLocalDriverConfig::from_loaded(&loaded)?,
        LinuxLocalDriverConfig::Sqlite { path } if path == dir.path().join("missing/vfs-driver-local.sqlite")
    ));
    assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
    Ok(())
}

#[test]
fn sqlite_paths_and_backend_variants_are_resolved_without_mutating_the_snapshot() -> TestResult {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("custom.toml");
    for (body, expected) in [
        (
            "backend = 'sqlite'\n",
            dir.path().join("vfs-driver-local.sqlite"),
        ),
        (
            "backend = 'sqlite'\npath = 'data/local.sqlite'\n",
            dir.path().join("data/local.sqlite"),
        ),
    ] {
        std::fs::write(&file, format!("[vfs.driver.local]\n{body}"))?;
        let loaded = Registry::for_space("default")?.load_required(&file)?;
        assert!(
            matches!(LinuxLocalDriverConfig::from_loaded(&loaded)?, LinuxLocalDriverConfig::Sqlite { path } if path == expected)
        );
        assert!(
            matches!(&*loaded.get::<LinuxLocalDriverConfig>()?, LinuxLocalDriverConfig::Sqlite { path } if path.is_relative())
        );
    }
    let absolute = dir.path().join("elsewhere/local.sqlite");
    std::fs::write(
        &file,
        format!(
            "[vfs.driver.local]\nbackend = 'sqlite'\npath = '{}'\n",
            absolute.display()
        ),
    )?;
    let loaded = Registry::for_space("default")?.load_required(&file)?;
    assert!(
        matches!(LinuxLocalDriverConfig::from_loaded(&loaded)?, LinuxLocalDriverConfig::Sqlite { path } if path == absolute)
    );
    std::fs::write(
        &file,
        "[vfs.driver.local]\nbackend = 'postgres'\nurl = 'postgresql://localhost/local'\n",
    )?;
    let loaded = Registry::for_space("default")?.load_required(&file)?;
    assert!(
        matches!(LinuxLocalDriverConfig::from_loaded(&loaded)?, LinuxLocalDriverConfig::Postgres { url } if url == "postgresql://localhost/local")
    );
    assert_eq!(std::fs::read_dir(dir.path())?.count(), 1);
    Ok(())
}

#[test]
fn invalid_local_settings_report_the_owned_section_without_secret_values() -> TestResult {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    for body in [
        "backend = 'unknown-secret'\n",
        "path = 'local.sqlite'\n",
        "backend = 'sqlite'\npath = 42\n",
        "backend = 'memory'\npath = 'local.sqlite'\n",
        "backend = 'postgres'\n",
        "backend = 'sqlite'\nurl = 'secret-url'\n",
        "backend = 'sqlite'\npath = ''\n",
        "backend = 'postgres'\nurl = '   '\n",
    ] {
        std::fs::write(&file, format!("[vfs.driver.local]\n{body}"))?;
        let error = Registry::for_space("default")?
            .load_required(&file)
            .err()
            .ok_or("expected invalid local settings")?;
        assert_eq!(error.section(), Some("vfs.driver.local"));
        assert!(matches!(
            error.kind(),
            ConfigErrorKind::Deserialize | ConfigErrorKind::Validation
        ));
        assert!(!error.to_string().contains("secret"));
    }
    Ok(())
}
