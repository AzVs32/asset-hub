use std::process::Command;

use asset_infra::repository::SqliteMountRepository;
use asset_vfs::driver::{DriverKind, DriverPath};
use asset_vfs::mount::{Mount, MountId, MountRepository};
use asset_vfs::namespace::VirtualPath;
use tempfile::tempdir;

fn write_config(path: &std::path::Path, root: &std::path::Path, storage: &std::path::Path) {
    std::fs::write(
        path,
        format!(
            "[asset]\nroot_mount_path = {:?}\nconfig_dir = {:?}\n",
            root.to_str().unwrap(),
            storage.to_str().unwrap(),
        ),
    )
    .unwrap();
}

fn configured_directory() -> tempfile::TempDir {
    let directory = tempdir().unwrap();
    write_config(
        &directory.path().join("config.toml"),
        &directory.path().join("data"),
        &directory.path().join("conf"),
    );
    directory
}

#[test]
fn help_and_invalid_arguments_do_not_initialize_storage() {
    let directory = tempdir().unwrap();
    for args in [
        vec!["--help"],
        vec!["mount", "info", "invalid-id"],
        vec!["mount", "resolve", "/bad//path"],
    ] {
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
fn configured_startup_creates_database_and_unknown_mount_fails() {
    let directory = configured_directory();
    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "list"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let listing = String::from_utf8(output.stdout).unwrap();
    let rows = table_rows(&listing);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0],
        [
            "VIRTUAL_PATH",
            "DRIVER",
            "DRIVER_PATH",
            "ENABLED",
            "ALLOWS_SUBMOUNTS",
        ]
    );
    let root = directory.path().join("data");
    assert_eq!(
        rows[1],
        ["/", "local", root.to_str().unwrap(), "true", "true"]
    );
    assert!(directory.path().join("conf/asset.db").is_file());
    assert!(directory.path().join("data").is_dir());

    let repeated = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "list"])
        .output()
        .unwrap();
    assert!(repeated.status.success());
    assert_eq!(String::from_utf8(repeated.stdout).unwrap(), listing);

    let info = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "info", "/"])
        .output()
        .unwrap();
    assert!(info.status.success());
    assert_eq!(String::from_utf8(info.stdout).unwrap(), listing);

    let with_ids = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "info", "/", "--show-ids"])
        .output()
        .unwrap();
    assert!(with_ids.status.success());
    let with_ids = String::from_utf8(with_ids.stdout).unwrap();
    let id_rows = table_rows(&with_ids);
    assert_eq!(id_rows[0][1], "ID");
    let id = id_rows[1][1];
    assert!(id.parse::<MountId>().is_ok());
    let by_id = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "info", id])
        .output()
        .unwrap();
    assert!(by_id.status.success());
    assert_eq!(String::from_utf8(by_id.stdout).unwrap(), with_ids);

    let other_directory = tempdir().unwrap();
    let from_other_directory = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(other_directory.path())
        .arg("--config")
        .arg(directory.path().join("config.toml"))
        .args(["mount", "info", "/", "--show-ids"])
        .output()
        .unwrap();
    assert!(
        from_other_directory.status.success(),
        "{from_other_directory:?}"
    );
    assert_eq!(
        String::from_utf8(from_other_directory.stdout).unwrap(),
        with_ids
    );
    assert!(!other_directory.path().join("data").exists());
    assert!(!other_directory.path().join("conf").exists());

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
fn driver_list_exposes_registered_kinds_and_submount_declarations() {
    let directory = configured_directory();
    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["driver", "list"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let listing = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        table_rows(&listing),
        [
            vec!["DRIVER", "ALLOWS_SUBMOUNTS"],
            vec!["local", "true"],
            vec!["memory", "false"],
        ]
    );
}

#[tokio::test]
async fn list_and_info_show_driver_policy_without_binding_or_changing_mounts() {
    let directory = configured_directory();
    let storage = directory.path().join("conf");
    std::fs::create_dir_all(&storage).unwrap();
    let repository = SqliteMountRepository::open(storage.join("asset.db"))
        .await
        .unwrap();
    let missing = directory.path().join("missing-directory");
    let cases = [
        (
            "/offline",
            "local",
            missing.to_str().unwrap(),
            false,
            "true",
        ),
        ("/memory", "memory", "invalid-memory-root", true, "false"),
        ("/unknown", "unavailable", "unknown-root", true, "unknown"),
    ];
    let mut mounts = Vec::new();
    for (virtual_path, kind, root, enabled, expected) in cases {
        let mount = Mount::new(
            MountId::new(),
            VirtualPath::try_from(virtual_path).unwrap(),
            DriverKind::try_from(kind).unwrap(),
            DriverPath::new(root),
            enabled,
        );
        repository.insert(&mount).await.unwrap();
        mounts.push((mount, expected));
    }

    let listing = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "list"])
        .output()
        .unwrap();
    assert!(listing.status.success(), "{listing:?}");
    let listing = String::from_utf8(listing.stdout).unwrap();
    let rows = table_rows(&listing);
    // Header, automatically created root mount, and the three stored mounts.
    assert_eq!(rows.len(), 5);
    for (mount, expected) in mounts {
        let path = mount.virtual_path().as_str();
        let row = rows.iter().find(|row| row[0] == path).unwrap();
        assert_eq!(row[4], expected);
        let info = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
            .current_dir(directory.path())
            .args(["mount", "info", path])
            .output()
            .unwrap();
        assert!(info.status.success(), "{info:?}");
        let info = String::from_utf8(info.stdout).unwrap();
        let info_rows = table_rows(&info);
        assert_eq!(info_rows.len(), 2);
        assert_eq!(info_rows[0], rows[0]);
        assert_eq!(&info_rows[1], row);
        assert_eq!(repository.get(mount.id()).await.unwrap(), Some(mount));
    }
    assert!(!directory.path().join("missing-directory").exists());
}

#[test]
fn custom_config_controls_storage_and_invalid_config_fails() {
    let directory = configured_directory();
    let config = directory.path().join("custom.toml");
    write_config(
        &config,
        &directory.path().join("data"),
        &directory.path().join("custom-conf"),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["mount", "list", "--config", "custom.toml"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(directory.path().join("custom-conf/asset.db").is_file());
    assert!(!directory.path().join("conf").exists());

    std::fs::write(&config, "[asset]\ndatabase = 'unknown'\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory.path())
        .args(["--config", "custom.toml", "mount", "list"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown"));
}

#[tokio::test]
async fn startup_updates_active_root_from_config_without_changing_id_or_other_mounts() {
    let directory = configured_directory();
    let storage = directory.path().join("conf");
    std::fs::create_dir_all(&storage).unwrap();
    let repository = SqliteMountRepository::open(storage.join("asset.db"))
        .await
        .unwrap();
    let root = Mount::new(
        MountId::new(),
        VirtualPath::root(),
        DriverKind::try_from("local").unwrap(),
        DriverPath::new(directory.path().join("old-root").to_str().unwrap()),
        true,
    );
    let other = Mount::new(
        MountId::new(),
        VirtualPath::try_from("/archive").unwrap(),
        DriverKind::try_from("local").unwrap(),
        DriverPath::new(directory.path().join("archive").to_str().unwrap()),
        true,
    );
    repository.insert(&root).await.unwrap();
    repository.insert(&other).await.unwrap();
    std::fs::create_dir(directory.path().join("old-root")).unwrap();
    std::fs::write(directory.path().join("old-root/keep.txt"), "keep").unwrap();

    for path in ["new-root", "changed-root"] {
        let root_path = directory.path().join(path);
        write_config(&directory.path().join("config.toml"), &root_path, &storage);
        let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
            .current_dir(directory.path())
            .args(["mount", "list"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let persisted = repository.get(root.id()).await.unwrap().unwrap();
        assert!(persisted.enabled());
        assert_eq!(persisted.virtual_path(), &VirtualPath::root());
        assert_eq!(
            persisted.driver_path().as_str(),
            root_path.to_str().unwrap()
        );
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
    for root in ["", "relative-root", "file-root"] {
        let directory = configured_directory();
        std::fs::write(directory.path().join("file-root"), "not a directory").unwrap();
        let root_path = if root == "file-root" {
            directory.path().join(root)
        } else {
            std::path::PathBuf::from(root)
        };
        write_config(
            &directory.path().join("config.toml"),
            &root_path,
            &directory.path().join("conf"),
        );
        let output = Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
            .current_dir(directory.path())
            .args(["mount", "list"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        if root != "file-root" {
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("asset.root_mount_path must be an absolute path")
            );
        }
        assert!(!directory.path().join("conf/asset.db").exists());
        assert!(!directory.path().join("relative-root").exists());
    }
}

#[test]
fn relative_or_empty_config_directory_is_rejected_before_creating_storage() {
    for storage in ["", "conf", "./conf", "../conf"] {
        let directory = configured_directory();
        write_config(
            &directory.path().join("config.toml"),
            &directory.path().join("data"),
            std::path::Path::new(storage),
        );
        let output = cli(directory.path(), &["mount", "list"]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("asset.config_dir must be an absolute path")
        );
        assert!(!directory.path().join("data").exists());
        assert!(!directory.path().join("conf").exists());
    }
}

#[tokio::test]
async fn local_mounts_require_absolute_paths_even_when_disabled() {
    let directory = configured_directory();
    // The relative directory exists; rejecting it must be a syntax decision.
    std::fs::create_dir(directory.path().join("data")).unwrap();
    for disabled in [false, true] {
        let mut args = vec!["mount", "add", "/relative", "local", "data"];
        if disabled {
            args.push("--disabled");
        }
        let output = cli(directory.path(), &args);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("driver path is invalid"));
    }
    let native = directory.path().join("data");
    let output = cli(
        directory.path(),
        &["mount", "add", "/s", "local", native.to_str().unwrap()],
    );
    assert!(output.status.success(), "{output:?}");
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(table_rows(&output)[1][2], native.to_str().unwrap());
    let missing = directory.path().join("missing");
    let output = cli(
        directory.path(),
        &[
            "mount",
            "add",
            "/offline",
            "local",
            missing.to_str().unwrap(),
            "--disabled",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(!missing.exists());
    let repository = SqliteMountRepository::open(directory.path().join("conf/asset.db"))
        .await
        .unwrap();
    let mounts = repository.list().await.unwrap();
    assert_eq!(mounts.len(), 3);
    assert!(
        !mounts
            .iter()
            .any(|mount| mount.virtual_path().as_str() == "/relative")
    );
}

fn cli(directory: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap()
}

#[tokio::test]
async fn mount_commands_use_paths_and_require_ids_only_for_disabled_ambiguity() {
    let directory = configured_directory();
    for index in 0..2 {
        let output = cli(
            directory.path(),
            &["mount", "add", "/cache", "memory", "/", "--disabled"],
        );
        assert!(output.status.success(), "{output:?}");
        let output = String::from_utf8(output.stdout).unwrap();
        assert_eq!(table_rows(&output)[0].contains(&"ID"), index == 1);
    }
    let active = cli(directory.path(), &["mount", "add", "/cache", "memory", "/"]);
    assert!(active.status.success(), "{active:?}");
    let duplicate = cli(directory.path(), &["mount", "add", "/cache", "memory", "/"]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("enabled mount already exists"));
    let child = cli(
        directory.path(),
        &["mount", "add", "/cache/child", "memory", "/"],
    );
    assert!(!child.status.success());
    assert!(String::from_utf8_lossy(&child.stderr).contains("does not allow submounts"));

    let listing = cli(directory.path(), &["mount", "list"]);
    assert!(listing.status.success());
    let listing = String::from_utf8(listing.stdout).unwrap();
    let rows = table_rows(&listing);
    assert_eq!(rows[0][1], "ID");
    assert_eq!(rows.len(), 5);
    let active_id = rows
        .iter()
        .find(|row| row[0] == "/cache" && row[4] == "true")
        .unwrap()[1]
        .to_owned();
    let info = cli(directory.path(), &["mount", "info", "/cache"]);
    assert!(info.status.success());
    let info = String::from_utf8(info.stdout).unwrap();
    let info_rows = table_rows(&info);
    assert!(!info_rows[0].contains(&"ID"));
    assert_eq!(info_rows[1][3], "true");

    let resolved = cli(directory.path(), &["mount", "resolve", "/cache/file"]);
    assert!(resolved.status.success());
    let resolved = String::from_utf8(resolved.stdout).unwrap();
    assert_eq!(table_rows(&resolved)[1], ["/cache", "file", "memory", "/"]);
    let unmounted = cli(directory.path(), &["mount", "unmount", "/cache"]);
    assert!(unmounted.status.success());
    let unmounted = String::from_utf8(unmounted.stdout).unwrap();
    let unmounted_rows = table_rows(&unmounted);
    assert_eq!(unmounted_rows[0][1], "ID");
    assert_eq!(unmounted_rows[1][1], active_id);
    let ambiguous = cli(directory.path(), &["mount", "info", "/cache"]);
    assert!(!ambiguous.status.success());
    assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("use a mount ID"));
    let ambiguous_unmount = cli(directory.path(), &["mount", "unmount", "/cache"]);
    assert!(!ambiguous_unmount.status.success());
    let fallback = cli(directory.path(), &["mount", "resolve", "/cache/file"]);
    assert!(fallback.status.success());
    let fallback = String::from_utf8(fallback.stdout).unwrap();
    assert_eq!(
        table_rows(&fallback)[1],
        [
            "/",
            "cache/file",
            "local",
            directory.path().join("data").to_str().unwrap()
        ]
    );
    let enabled = cli(directory.path(), &["mount", "enable", &active_id]);
    assert!(enabled.status.success(), "{enabled:?}");
    let repository = SqliteMountRepository::open(directory.path().join("conf/asset.db"))
        .await
        .unwrap();
    assert_eq!(repository.list().await.unwrap().len(), 4);
    assert!(
        repository
            .get(active_id.parse().unwrap())
            .await
            .unwrap()
            .unwrap()
            .enabled()
    );
    let root = cli(directory.path(), &["mount", "unmount", "/"]);
    assert!(!root.status.success());
    assert!(String::from_utf8_lossy(&root.stderr).contains("root mount must be preserved"));
}

#[tokio::test]
async fn startup_applies_service_topology_validation_before_saving_root() {
    let directory = configured_directory();
    std::fs::create_dir(directory.path().join("conf")).unwrap();
    let repository = SqliteMountRepository::open(directory.path().join("conf/asset.db"))
        .await
        .unwrap();
    // Repository writes enforce storage constraints, while the service owns
    // the rule prohibiting enabled descendants of a memory mount.
    for path in ["/cache", "/cache/child"] {
        repository
            .insert(&Mount::new(
                MountId::new(),
                VirtualPath::try_from(path).unwrap(),
                DriverKind::try_from("memory").unwrap(),
                DriverPath::new("/"),
                true,
            ))
            .await
            .unwrap();
    }
    let before = repository.list().await.unwrap();
    let output = cli(directory.path(), &["mount", "list"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not allow submounts"));
    assert_eq!(repository.list().await.unwrap(), before);
}

#[tokio::test]
async fn concurrent_startup_reuses_one_enabled_root() {
    let directory = configured_directory();
    std::fs::create_dir(directory.path().join("conf")).unwrap();
    let repository = SqliteMountRepository::open(directory.path().join("conf/asset.db"))
        .await
        .unwrap();
    let processes: Vec<_> = (0..8)
        .map(|_| {
            Command::new(env!("CARGO_BIN_EXE_asset-cli-test"))
                .current_dir(directory.path())
                .args(["mount", "info", "/", "--show-ids"])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let mut ids = Vec::new();
    for process in processes {
        let output = process.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let output = String::from_utf8(output.stdout).unwrap();
        ids.push(table_rows(&output)[1][1].to_owned());
    }
    let definitions = repository.list().await.unwrap();
    assert_eq!(definitions.len(), 1);
    assert!(definitions[0].enabled());
    assert_eq!(definitions[0].virtual_path(), &VirtualPath::root());
    assert!(ids.iter().all(|id| *id == definitions[0].id().to_string()));
}

#[tokio::test]
async fn startup_uses_only_enabled_roots_and_leaves_disabled_definitions_unchanged() {
    for disabled_count in 1..=2 {
        let directory = configured_directory();
        std::fs::create_dir(directory.path().join("conf")).unwrap();
        let repository = SqliteMountRepository::open(directory.path().join("conf/asset.db"))
            .await
            .unwrap();
        let disabled = ["disabled-a", "disabled-b"].map(|path| {
            Mount::new(
                MountId::new(),
                VirtualPath::root(),
                DriverKind::try_from("local").unwrap(),
                DriverPath::new(directory.path().join(path).to_str().unwrap()),
                false,
            )
        });
        for mount in &disabled[..disabled_count] {
            repository.insert(mount).await.unwrap();
        }
        let output = cli(directory.path(), &["mount", "list"]);
        assert!(output.status.success(), "{output:?}");
        let definitions = repository.list().await.unwrap();
        assert_eq!(definitions.len(), disabled_count + 1);
        let active_id = definitions
            .iter()
            .find(|mount| mount.enabled())
            .unwrap()
            .id();
        for mount in &disabled[..disabled_count] {
            assert_eq!(
                repository.get(mount.id()).await.unwrap(),
                Some(mount.clone())
            );
        }
        let repeated = cli(directory.path(), &["mount", "info", "/"]);
        assert!(repeated.status.success());
        let definitions = repository.list().await.unwrap();
        assert_eq!(definitions.len(), disabled_count + 1);
        assert_eq!(
            definitions
                .iter()
                .find(|mount| mount.enabled())
                .unwrap()
                .id(),
            active_id
        );
    }
}
