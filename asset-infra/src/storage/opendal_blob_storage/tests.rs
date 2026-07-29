use super::*;
use std::path::PathBuf;

#[tokio::test]
async fn fs_storage_roundtrips_blob_content() {
    let storage = storage("fs-roundtrip");
    let key = StorageKey::new("assets/image.png").unwrap();
    let data = Bytes::from_static(b"image bytes");

    storage.put(&key, data.clone()).await.unwrap();

    assert_eq!(storage.get(&key).await.unwrap(), Some(data));

    storage.delete(&key).await.unwrap();

    assert_eq!(storage.get(&key).await.unwrap(), None);
}

#[tokio::test]
async fn fs_storage_preserves_spaces_in_the_physical_path() {
    let (storage, root) = storage_with_root("fs-path-spaces");
    let key = StorageKey::new(" library / project A / design  01.md ").unwrap();
    let data = Bytes::from_static(b"draft");
    let stream: BlobByteStream = Box::pin(futures_util::stream::once({
        let data = data.clone();
        async move { Ok(data) }
    }));

    stage_and_publish(&storage, &key, stream).await;

    assert_eq!(storage.get(&key).await.unwrap(), Some(data.clone()));
    assert_eq!(std::fs::read(root.join(key.as_str())).unwrap(), data);
    assert!(!root.join("library/project A/design  01.md").exists());

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn fs_storage_delete_removes_empty_sidecar_directories() {
    let (storage, root) = storage_with_root("fs-clean-sidecar");
    let key = StorageKey::new(format!(
        ".asset-hub/action-effects/action-replacements/{}",
        uuid::Uuid::now_v7()
    ))
    .unwrap();

    storage
        .put(&key, Bytes::from_static(b"temporary"))
        .await
        .unwrap();
    storage.delete(&key).await.unwrap();

    assert!(!root.join(".asset-hub").exists());
}

#[tokio::test]
async fn fs_storage_ensures_each_user_directory_segment() {
    let (storage, root) = storage_with_root("fs-ensure-directory");
    let directory = DirectoryPath::from_path("projects/design/assets").unwrap();

    storage.ensure_directory(&directory).await.unwrap();

    assert!(root.join("projects").is_dir());
    assert!(root.join("projects/design").is_dir());
    assert!(root.join("projects/design/assets").is_dir());
    storage.ensure_directory(&directory).await.unwrap();
}

#[tokio::test]
async fn fs_storage_moves_a_complete_directory_subtree() {
    let (storage, root) = storage_with_root("fs-move-directory");
    let source = DirectoryPath::from_path("games/title").unwrap();
    let destination = DirectoryPath::from_path("archive/renamed").unwrap();
    storage.ensure_directory(&source).await.unwrap();
    std::fs::write(root.join("games/title/game.dat"), b"game").unwrap();

    storage.move_directory(&source, &destination).await.unwrap();

    assert!(!root.join("games/title").exists());
    assert_eq!(
        std::fs::read(root.join("archive/renamed/game.dat")).unwrap(),
        b"game"
    );
}

#[tokio::test]
async fn fs_blob_delete_preserves_empty_user_directories() {
    let (storage, root) = storage_with_root("fs-preserve-user-directory");
    let key = StorageKey::new("drafts/readme.md").unwrap();
    storage
        .put(&key, Bytes::from_static(b"draft"))
        .await
        .unwrap();

    storage.delete(&key).await.unwrap();

    assert!(root.join("drafts").is_dir());
}

#[tokio::test]
async fn fs_storage_writes_streaming_blob_content() {
    let storage = storage("fs-stream");
    let key = StorageKey::new("assets/large.bin").unwrap();
    let stream: BlobByteStream = Box::pin(futures_util::stream::iter([
        Ok(Bytes::from_static(b"large ")),
        Ok(Bytes::from_static(b"file ")),
        Ok(Bytes::from_static(b"bytes")),
    ]));

    let staged = stage_and_publish(&storage, &key, stream).await;

    assert_eq!(staged.bytes_written(), 16);
    assert_eq!(
        storage.get(&key).await.unwrap(),
        Some(Bytes::from_static(b"large file bytes"))
    );
}

#[tokio::test]
async fn fs_storage_stages_complete_content_before_atomic_publish() {
    let (storage, root) = storage_with_root("fs-staged-publish");
    let key = StorageKey::new("assets/large.bin").unwrap();
    let stream: BlobByteStream = Box::pin(futures_util::stream::iter([
        Ok(Bytes::from_static(b"large ")),
        Ok(Bytes::from_static(b"file")),
    ]));

    let staged = storage.stage_stream(stream).await.unwrap();

    assert!(!root.join(key.as_str()).exists());
    assert_eq!(
        std::fs::read(root.join(staged.key().as_str())).unwrap(),
        b"large file"
    );

    storage
        .publish_staged_if_absent(&staged, &key)
        .await
        .unwrap();

    assert_eq!(
        std::fs::read(root.join(key.as_str())).unwrap(),
        b"large file"
    );
    assert!(root.join(staged.key().as_str()).exists());

    storage.discard_staged(&staged).await.unwrap();

    assert!(!root.join(staged.key().as_str()).exists());
    assert_eq!(
        std::fs::read(root.join(key.as_str())).unwrap(),
        b"large file"
    );
}

#[tokio::test]
async fn fs_storage_removes_partial_staging_file_after_stream_failure() {
    let (storage, root) = storage_with_root("fs-staged-failure");
    let stream: BlobByteStream = Box::pin(futures_util::stream::iter([
        Ok(Bytes::from_static(b"partial")),
        Err(CoreError::configuration("input failed")),
    ]));

    assert!(storage.stage_stream(stream).await.is_err());

    let uploads = root.join(".asset-hub/uploads");
    assert!(!uploads.exists() || std::fs::read_dir(uploads).unwrap().next().is_none());
}

#[tokio::test]
async fn fs_storage_streams_blob_byte_range() {
    let storage = storage("fs-range-stream");
    let key = StorageKey::new("assets/video.mp4").unwrap();
    storage
        .put(&key, Bytes::from_static(b"0123456789"))
        .await
        .unwrap();

    let stream = storage.get_range_stream(&key, 2, 5).await.unwrap().unwrap();
    let chunks = stream.try_collect::<Vec<_>>().await.unwrap();
    let bytes = chunks.into_iter().fold(Vec::new(), |mut bytes, chunk| {
        bytes.extend_from_slice(&chunk);
        bytes
    });

    assert_eq!(bytes, b"2345");
}

#[tokio::test]
async fn fs_storage_atomic_publish_rejects_existing_blob() {
    let storage = storage("fs-stream-if-absent");
    let key = StorageKey::new("assets/large.bin").unwrap();
    let first: BlobByteStream = Box::pin(futures_util::stream::iter([Ok(Bytes::from_static(
        b"first",
    ))]));
    let second: BlobByteStream = Box::pin(futures_util::stream::iter([Ok(Bytes::from_static(
        b"second",
    ))]));

    stage_and_publish(&storage, &key, first).await;
    let staged = storage.stage_stream(second).await.unwrap();
    let error = storage
        .publish_staged_if_absent(&staged, &key)
        .await
        .unwrap_err();
    storage.discard_staged(&staged).await.unwrap();

    match error {
        CoreError::Conflict { message } => assert!(message.contains("already exists")),
        other => panic!("expected conflict, got {other:?}"),
    }
    assert_eq!(
        storage.get(&key).await.unwrap(),
        Some(Bytes::from_static(b"first"))
    );
}

#[tokio::test]
async fn fs_storage_moves_blob_without_overwriting_target() {
    let (storage, root) = storage_with_root("fs-move-if-absent");
    let source = StorageKey::new("drafts/readme.md").unwrap();
    let target = StorageKey::new("docs/readme.md").unwrap();
    storage
        .put(&source, Bytes::from_static(b"source"))
        .await
        .unwrap();

    storage.move_if_absent(&source, &target).await.unwrap();

    assert_eq!(storage.get(&source).await.unwrap(), None);
    assert_eq!(
        storage.get(&target).await.unwrap(),
        Some(Bytes::from_static(b"source"))
    );
    assert!(root.join("drafts").is_dir());

    let another = StorageKey::new("incoming/readme.md").unwrap();
    storage
        .put(&another, Bytes::from_static(b"another"))
        .await
        .unwrap();
    let error = storage.move_if_absent(&another, &target).await.unwrap_err();

    assert!(matches!(error, CoreError::Conflict { .. }));
    assert_eq!(
        storage.get(&another).await.unwrap(),
        Some(Bytes::from_static(b"another"))
    );
    assert_eq!(
        storage.get(&target).await.unwrap(),
        Some(Bytes::from_static(b"source"))
    );
}

fn storage(name: &str) -> OpenDalBlobStorage {
    storage_with_root(name).0
}

async fn stage_and_publish(
    storage: &OpenDalBlobStorage,
    key: &StorageKey,
    stream: BlobByteStream,
) -> StagedBlob {
    let staged = storage.stage_stream(stream).await.unwrap();
    storage
        .publish_staged_if_absent(&staged, key)
        .await
        .unwrap();
    storage.discard_staged(&staged).await.unwrap();
    staged
}

fn storage_with_root(name: &str) -> (OpenDalBlobStorage, PathBuf) {
    let root = unique_temp_path(name);
    std::fs::create_dir_all(&root).unwrap();
    let storage = OpenDalBlobStorage::from_local_root(&root).unwrap();

    (storage, root)
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
