use std::process::Command;

use asset_infra::repository::SqliteMountRepository;
use asset_vfs::driver::{DriverKind, DriverPath};
use asset_vfs::mount::{Mount, MountId, MountRepository};
use asset_vfs::namespace::VirtualPath;
use tempfile::tempdir;

#[test]
fn help_and_invalid_arguments_do_not_initialize_storage() {
    let directory = tempdir().unwrap();
    for args in [vec!["--help"], vec!["mount", "info", "invalid-id"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
            .current_dir(directory.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), args == ["--help"]);
        assert!(!directory.path().join("conf").exists());
    }
}

#[test]
fn default_startup_creates_database_and_unknown_mount_fails() {
    let directory = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "list"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let listing = String::from_utf8(output.stdout).unwrap();
    let rows = table_rows(&listing);
    assert_eq!(rows.len(), 2);
    assert_eq!(&rows[1][1..], ["/", "local", "data", "true"]);
    assert!(directory.path().join("conf/vfs.db").is_file());
    assert!(directory.path().join("data").is_dir());

    let repeated = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "list"])
        .output()
        .unwrap();
    assert!(repeated.status.success());
    assert_eq!(String::from_utf8(repeated.stdout).unwrap(), listing);

    let id = rows[1][0];
    let info = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "info", id])
        .output()
        .unwrap();
    assert!(info.status.success());
    assert_eq!(String::from_utf8(info.stdout).unwrap(), listing);

    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args([
            "mount",
            "info",
            &asset_vfs::mount::MountId::new().to_string(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("mount not found"));
}

fn table_rows(output: &str) -> Vec<Vec<&str>> {
    output
        .lines()
        .filter(|line| line.starts_with('│'))
        .map(|line| line.trim_matches('│').split('┆').map(str::trim).collect())
        .collect()
}

#[test]
fn custom_config_controls_storage_and_invalid_config_fails() {
    let directory = tempdir().unwrap();
    let config = directory.path().join("custom.toml");
    std::fs::write(&config, "[vfs]\nconfig_dir = 'custom-conf'\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "list", "--config", "custom.toml"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(directory.path().join("custom-conf/vfs.db").is_file());
    assert!(!directory.path().join("conf").exists());

    std::fs::write(&config, "[vfs]\ndatabase = 'unknown'\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["--config", "custom.toml", "mount", "list"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown"));
}

#[tokio::test]
async fn startup_repairs_root_from_config_without_changing_id_or_other_mounts() {
    let directory = tempdir().unwrap();
    let storage = directory.path().join("conf");
    std::fs::create_dir_all(&storage).unwrap();
    let repository = SqliteMountRepository::open(storage.join("vfs.db"))
        .await
        .unwrap();
    let root = Mount::new(
        MountId::new(),
        VirtualPath::root(),
        DriverKind::try_from("local").unwrap(),
        DriverPath::new("old-root"),
        false,
    );
    let other = Mount::new(
        MountId::new(),
        VirtualPath::try_from("/archive").unwrap(),
        DriverKind::try_from("local").unwrap(),
        DriverPath::new("archive"),
        true,
    );
    repository.insert(&root).await.unwrap();
    repository.insert(&other).await.unwrap();
    std::fs::create_dir(directory.path().join("old-root")).unwrap();
    std::fs::write(directory.path().join("old-root/keep.txt"), "keep").unwrap();

    for path in ["new-root", "changed-root"] {
        std::fs::write(
            directory.path().join("config.toml"),
            format!("[vfs]\nroot_mount_path = '{path}'\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
            .current_dir(directory.path())
            .args(["mount", "list"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let persisted = repository.get(root.id()).await.unwrap().unwrap();
        assert!(persisted.enabled());
        assert_eq!(persisted.virtual_path(), &VirtualPath::root());
        assert_eq!(persisted.driver_path().as_str(), path);
        assert_eq!(persisted.driver().as_str(), "local");
        assert!(directory.path().join(path).is_dir());
        assert_eq!(
            repository.get(other.id()).await.unwrap(),
            Some(other.clone())
        );
        assert_eq!(repository.list().await.unwrap().len(), 2);
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("old-root/keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn invalid_root_prevents_successful_startup() {
    for root in ["", "file-root"] {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("file-root"), "not a directory").unwrap();
        std::fs::write(
            directory.path().join("config.toml"),
            format!("[vfs]\nroot_mount_path = '{root}'\n"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
            .current_dir(directory.path())
            .args(["mount", "list"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!directory.path().join("conf/vfs.db").exists());
    }
}
