use config_extension as _;
use std::sync::Arc;

#[derive(settings::Config)]
#[config(key = "host", spaces = ["default", "plugins"], default = host_defaults())]
struct HostConfig {
    message: String,
    #[config(default)]
    mode: Mode,
}

fn host_defaults() -> HostConfig {
    HostConfig {
        message: "default".into(),
        mode: Mode::Safe,
    }
}

#[derive(settings::ConfigValue, Debug, PartialEq)]
#[config(default = Self::Safe)]
enum Mode {
    Safe,
    Fast,
}

#[cfg(any())]
#[derive(settings::Config)]
#[config(key = "host", spaces = ["plugins"])]
struct DisabledConfig {
    value: MissingType,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let defaults: HostConfig = Default::default();
    assert_eq!(defaults.message, "default");
    assert_eq!(defaults.mode, Mode::Safe);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "[host]\nmessage='loaded'\n[extension]\nport=9000")?;
    let host = settings::Registry::for_space("default")?.load_required(&path)?;
    let plugins = settings::Registry::for_space("plugins")?.load_required(&path)?;
    assert_eq!(host.get::<HostConfig>()?.message, "loaded");
    assert_eq!(host.get::<HostConfig>()?.mode, Mode::Safe);
    assert_eq!(plugins.get::<HostConfig>()?.message, "loaded");
    assert!(!Arc::ptr_eq(
        &host.get::<HostConfig>()?,
        &plugins.get::<HostConfig>()?
    ));
    let extension = settings::Registry::for_space("extension_only")?.load_required(&path)?;
    assert_eq!(
        extension
            .get::<HostConfig>()
            .err()
            .map(|error| error.kind()),
        Some(settings::ConfigErrorKind::NotRegistered)
    );

    // An unselected extension is ignored; the same input fails when it is selected.
    std::fs::write(
        &path,
        "[host]\nmessage='loaded'\n[extension]\nport='invalid'",
    )?;
    settings::Registry::for_space("default")?.load_required(&path)?;
    assert_eq!(
        settings::Registry::for_space("plugins")?
            .load_required(&path)
            .err()
            .map(|error| error.kind()),
        Some(settings::ConfigErrorKind::Deserialize)
    );
    println!("Renamed dependency and linked extension declarations verified.");
    Ok(())
}
