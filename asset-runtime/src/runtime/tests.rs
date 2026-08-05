use super::*;
use asset_core::domain::{DirectoryPath, Resource};
use asset_infra::config::{
    BlobConfig, DatabaseConfig, LocalBlobConfig, LocalBlobSyncConfig, SqliteDatabaseConfig,
};
use std::time::Duration;

async fn wait_for_resource(
    runtime: &AssetRuntime,
    directory: &DirectoryPath,
    name: &str,
) -> Option<Resource> {
    for _ in 0..100 {
        if let Some(resource) = runtime
            .infrastructure
            .resource_query()
            .find_by_path(directory, name)
            .await
            .unwrap()
        {
            return Some(resource.into_resource());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

async fn wait_until_absent(runtime: &AssetRuntime, directory: &DirectoryPath, name: &str) {
    for _ in 0..100 {
        if runtime
            .infrastructure
            .resource_query()
            .find_by_path(directory, name)
            .await
            .unwrap()
            .is_none()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("resource `{directory}/{name}` was not removed by automatic synchronization");
}

#[tokio::test(flavor = "multi_thread")]
async fn local_storage_changes_are_synchronized_automatically() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "asset-hub-auto-sync-{}-{nonce}",
        std::process::id()
    ));
    let config = AssetInfraConfig {
        database: DatabaseConfig {
            sqlite: SqliteDatabaseConfig { max_connections: 1 },
            ..DatabaseConfig::default()
        },
        blob: BlobConfig {
            local: LocalBlobConfig {
                root: root.clone(),
                sync: LocalBlobSyncConfig {
                    enabled: true,
                    debounce_milliseconds: 50,
                    reconcile_interval_seconds: 1,
                },
            },
            ..BlobConfig::default()
        },
        ..AssetInfraConfig::default()
    };
    let mut runtime = AssetRuntime::new(config).await.unwrap();
    runtime.start_storage_sync().await.unwrap();
    let directory = DirectoryPath::from_path("documents").unwrap();
    let directory_path = root.join("documents");
    std::fs::create_dir_all(&directory_path).unwrap();
    std::fs::write(directory_path.join("note.txt"), b"first").unwrap();

    let first = wait_for_resource(&runtime, &directory, "note.txt")
        .await
        .expect("new file should be imported automatically");
    std::fs::write(directory_path.join("note.txt"), b"second version").unwrap();
    let mut updated = None;
    for _ in 0..100 {
        let resource = runtime
            .infrastructure
            .resource_query()
            .find_by_path(&directory, "note.txt")
            .await
            .unwrap()
            .unwrap();
        if resource.resource().content().unwrap().size() == 14 {
            updated = Some(resource);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let updated = updated.expect("modified file should update Resource content");
    assert_eq!(updated.resource().id(), first.id());

    std::fs::rename(
        directory_path.join("note.txt"),
        directory_path.join("renamed.txt"),
    )
    .unwrap();
    let renamed = wait_for_resource(&runtime, &directory, "renamed.txt")
        .await
        .expect("renamed file should be synchronized automatically");
    assert_eq!(renamed.id(), first.id());

    std::fs::remove_file(directory_path.join("renamed.txt")).unwrap();
    wait_until_absent(&runtime, &directory, "renamed.txt").await;

    let empty_directory = root.join("empty");
    std::fs::create_dir(&empty_directory).unwrap();
    let mut directory_created = false;
    for _ in 0..100 {
        if runtime
            .infrastructure
            .directory_index()
            .list_children(&asset_core::domain::DirectoryId::root())
            .await
            .unwrap()
            .iter()
            .any(|directory| directory.path().path() == "empty")
        {
            directory_created = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(directory_created, "new directory should be synchronized");
    std::fs::remove_dir(&empty_directory).unwrap();
    let mut directory_removed = false;
    for _ in 0..100 {
        if runtime
            .infrastructure
            .directory_index()
            .list_children(&asset_core::domain::DirectoryId::root())
            .await
            .unwrap()
            .iter()
            .all(|directory| directory.path().path() != "empty")
        {
            directory_removed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        directory_removed,
        "removed directory should be synchronized"
    );

    drop(runtime);
    tokio::task::yield_now().await;
    let _ = std::fs::remove_dir_all(&root);
}
