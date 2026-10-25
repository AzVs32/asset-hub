#![cfg(feature = "derive")]

use ah_config::{ConfigErrorKind, Registry};
use std::{error::Error, path::PathBuf, process::Command};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(ah_config::Config)]
#[config(key = "application", spaces = ["paths"])]
struct ApplicationConfig {
    #[config(default = "default".into())]
    name: String,
    #[config(default = PathBuf::from("data"))]
    data_dir: PathBuf,
}

#[test]
fn custom_base_loads_relative_files_without_rewriting_configuration_values() -> TestResult {
    let directory = tempfile::tempdir()?;
    std::fs::create_dir(directory.path().join("config"))?;
    let path = directory.path().join("config/application.toml");
    std::fs::write(
        &path,
        "[application]\nname='configured'\ndata_dir='storage'",
    )?;
    let loaded = Registry::for_space("paths")?
        .with_base_dir(directory.path())
        .load_required("config/application.toml")?;
    assert_eq!(loaded.path(), path);
    assert_eq!(loaded.get::<ApplicationConfig>()?.name, "configured");
    assert_eq!(
        loaded.get::<ApplicationConfig>()?.data_dir,
        PathBuf::from("storage")
    );
    Ok(())
}

#[test]
fn absolute_files_bypass_even_an_invalid_base() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("application.toml");
    std::fs::write(&path, "[application]\nname='absolute'")?;
    for registry in [
        Registry::for_space("paths")?,
        Registry::for_space("paths")?.with_base_dir("relative/base"),
    ] {
        let loaded = registry.load_required(&path)?;
        assert_eq!(loaded.path(), path);
        assert_eq!(loaded.get::<ApplicationConfig>()?.name, "absolute");
    }
    Ok(())
}

#[test]
fn missing_files_obey_the_same_base_for_optional_and_required_loads() -> TestResult {
    let directory = tempfile::tempdir()?;
    let loaded = Registry::for_space("paths")?
        .with_base_dir(directory.path())
        .load_optional("missing.toml")?;
    assert_eq!(loaded.path(), directory.path().join("missing.toml"));
    assert_eq!(loaded.get::<ApplicationConfig>()?.name, "default");
    assert_eq!(
        Registry::for_space("paths")?
            .with_base_dir(directory.path())
            .load_required("missing.toml")
            .err()
            .map(|error| error.kind()),
        Some(ConfigErrorKind::MissingFile)
    );
    Ok(())
}

#[test]
fn invalid_relative_paths_and_bases_fail_even_for_optional_files() -> TestResult {
    for base in ["", "relative/base"] {
        let error = Registry::for_space("paths")?
            .with_base_dir(base)
            .load_optional("application.toml")
            .err()
            .ok_or("expected an invalid base error")?;
        assert_eq!(error.kind(), ConfigErrorKind::Read);
        let cause = error
            .source()
            .and_then(Error::source)
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .ok_or("expected an I/O cause")?;
        assert_eq!(cause.kind(), std::io::ErrorKind::InvalidInput);
    }
    assert_eq!(
        Registry::for_space("paths")?
            .load_optional("")
            .err()
            .map(|error| error.kind()),
        Some(ConfigErrorKind::Read)
    );
    Ok(())
}

#[test]
fn paths_are_independent_of_the_process_working_directory() -> TestResult {
    // Isolate environment and working-directory changes from parallel tests.
    const CHILD_BASE: &str = "AH_CONFIG_PATH_TEST_BASE";
    const CHILD_MODE: &str = "AH_CONFIG_PATH_TEST_MODE";
    if let Some(base) = std::env::var_os(CHILD_BASE) {
        return check_child_paths(PathBuf::from(base), &std::env::var(CHILD_MODE)?);
    }

    let base = tempfile::tempdir()?;
    std::fs::create_dir(base.path().join("config"))?;
    std::fs::write(
        base.path().join("config/config.toml"),
        "[application]\nname='from-base'",
    )?;
    let cwd = tempfile::tempdir()?;
    std::fs::create_dir(cwd.path().join("config"))?;
    std::fs::write(
        cwd.path().join("config/config.toml"),
        "[application]\nname='from-cwd'",
    )?;
    // A unique name prevents unrelated files beside the test executable from interfering.
    let missing = base
        .path()
        .file_name()
        .ok_or("temporary directory has no name")?;
    std::fs::write(cwd.path().join(missing), "[application]\nname='from-cwd'")?;
    for (mode, manifest_dir) in [
        ("cargo", Some(base.path())),
        ("direct", None),
        ("invalid", Some(std::path::Path::new("relative/base"))),
        ("empty", Some(std::path::Path::new(""))),
    ] {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args([
                "--exact",
                "paths_are_independent_of_the_process_working_directory",
            ])
            .env(CHILD_BASE, base.path())
            .env(CHILD_MODE, mode)
            .current_dir(cwd.path());
        match manifest_dir {
            Some(directory) => command.env("CARGO_MANIFEST_DIR", directory),
            None => command.env_remove("CARGO_MANIFEST_DIR"),
        };
        let output = command.output()?;
        assert!(
            output.status.success(),
            "child mode {mode} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn check_child_paths(base: PathBuf, mode: &str) -> TestResult {
    let path = base.join("config/config.toml");
    // Explicit bases and absolute file paths override even invalid environment values.
    let explicit = Registry::for_space("paths")?
        .with_base_dir(&base)
        .load_required("config/config.toml")?;
    let absolute = Registry::for_space("paths")?.load_required(&path)?;
    for loaded in [explicit, absolute] {
        assert_eq!(loaded.path(), path);
        assert_eq!(loaded.get::<ApplicationConfig>()?.name, "from-base");
    }
    match mode {
        "cargo" => {
            let loaded = Registry::for_space("paths")?.load_required("config/config.toml")?;
            assert_eq!(loaded.path(), path);
            assert_eq!(loaded.get::<ApplicationConfig>()?.name, "from-base");
            let missing = Registry::for_space("paths")?.load_optional("missing.toml")?;
            assert_eq!(missing.path(), base.join("missing.toml"));
            assert_eq!(missing.get::<ApplicationConfig>()?.name, "default");
        }
        "direct" => {
            let executable = std::env::current_exe()?;
            let directory = executable.parent().ok_or("executable has no parent")?;
            let missing = base.file_name().ok_or("temporary directory has no name")?;
            let loaded = Registry::for_space("paths")?.load_optional(missing)?;
            assert_eq!(loaded.path(), directory.join(missing));
            assert_eq!(loaded.get::<ApplicationConfig>()?.name, "default");
            assert_eq!(
                Registry::for_space("paths")?
                    .load_required(missing)
                    .err()
                    .map(|error| error.kind()),
                Some(ConfigErrorKind::MissingFile)
            );
        }
        "invalid" | "empty" => {
            assert_eq!(
                Registry::for_space("paths")?
                    .load_optional("config/config.toml")
                    .err()
                    .map(|error| error.kind()),
                Some(ConfigErrorKind::Read)
            );
        }
        _ => return Err("unknown child test mode".into()),
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn drive_relative_and_rooted_paths_are_rejected() -> TestResult {
    for path in [r"C:config.toml", r"\config.toml"] {
        assert_eq!(
            Registry::for_space("paths")?
                .load_optional(path)
                .err()
                .map(|error| error.kind()),
            Some(ConfigErrorKind::Read)
        );
    }
    Ok(())
}
