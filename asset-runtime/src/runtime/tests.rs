use super::*;
use asset_core::domain::{
    AccessContext, DirectoryKind, DirectoryPath, Resource, ResourceKind, UserId,
};
use asset_core::port::ListResources;
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
        if let Some(resource) = find_resource(runtime, directory, name).await {
            return Some(resource);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

async fn wait_until_absent(runtime: &AssetRuntime, directory: &DirectoryPath, name: &str) {
    for _ in 0..100 {
        if find_resource(runtime, directory, name).await.is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("resource `{directory}/{name}` was not removed by automatic synchronization");
}

async fn find_resource(
    runtime: &AssetRuntime,
    directory: &DirectoryPath,
    name: &str,
) -> Option<Resource> {
    let service = runtime.resource_service();
    let authorization = runtime.authorization_service();
    let context = AccessContext::administrator(UserId::new());
    let page = service
        .secured(&authorization, &context)
        .list_resources(ListResources::new(100, 0).with_directory(directory.clone()))
        .await
        .ok()?;
    page.items
        .into_iter()
        .find(|located| located.resource().name() == name)
        .map(|located| located.into_resource())
}

async fn root_directory_paths(runtime: &AssetRuntime) -> Vec<DirectoryPath> {
    let service = runtime.resource_service();
    let directories = service.directory_service();
    let root = directories.root().await.unwrap();
    directories
        .list_children(&root)
        .await
        .unwrap()
        .into_iter()
        .map(|directory| directory.path().clone())
        .collect()
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
    let service = runtime.resource_service();
    assert!(!service.kind_definitions().is_empty());
    assert!(
        !service
            .describe_kind_actions(&ResourceKind::default())
            .is_empty()
    );
    assert!(!service.directory_service().kind_definitions().is_empty());
    assert!(
        !service
            .directory_service()
            .describe_kind_actions(&DirectoryKind::default())
            .is_empty()
    );
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
        let resource = find_resource(&runtime, &directory, "note.txt")
            .await
            .unwrap();
        if resource.content().unwrap().size() == 14 {
            updated = Some(resource);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let updated = updated.expect("modified file should update Resource content");
    assert_eq!(updated.id(), first.id());

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
        if root_directory_paths(&runtime)
            .await
            .iter()
            .any(|path| path.path() == "empty")
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
        if root_directory_paths(&runtime)
            .await
            .iter()
            .all(|path| path.path() != "empty")
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
