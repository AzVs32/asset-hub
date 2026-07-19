use super::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct WriterTestError(&'static str);
impl std::fmt::Display for WriterTestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for WriterTestError {}

struct FailingWriter {
    fail_write: bool,
    fail_close: bool,
    fail_abort: bool,
    aborts: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl StreamWriter for FailingWriter {
    async fn write(&mut self, _chunk: Bytes) -> Result<(), WriterError> {
        if self.fail_write {
            Err(Box::new(WriterTestError("write")))
        } else {
            Ok(())
        }
    }
    async fn close(&mut self) -> Result<(), WriterError> {
        if self.fail_close {
            Err(Box::new(WriterTestError("close")))
        } else {
            Ok(())
        }
    }
    async fn abort(&mut self) -> Result<(), WriterError> {
        self.aborts.fetch_add(1, Ordering::Relaxed);
        if self.fail_abort {
            Err(Box::new(WriterTestError("abort")))
        } else {
            Ok(())
        }
    }
}

fn failing_writer(
    fail_write: bool,
    fail_close: bool,
    fail_abort: bool,
) -> (FailingWriter, Arc<AtomicUsize>) {
    let aborts = Arc::new(AtomicUsize::new(0));
    (
        FailingWriter {
            fail_write,
            fail_close,
            fail_abort,
            aborts: aborts.clone(),
        },
        aborts,
    )
}

#[tokio::test]
async fn stream_writer_aborts_after_write_or_close_failure() {
    for (fail_write, fail_close) in [(true, false), (false, true)] {
        let (writer, aborts) = failing_writer(fail_write, fail_close, true);
        let stream: BlobByteStream = Box::pin(futures_util::stream::once(async {
            Ok(Bytes::from_static(b"data"))
        }));
        let error = put_stream_with_writer(writer, stream).await.unwrap_err();
        let expected_operation = if fail_write {
            "put_stream.write"
        } else {
            "put_stream.close"
        };
        assert!(
            matches!(error, CoreError::Storage { operation, .. } if operation == expected_operation)
        );
        assert_eq!(aborts.load(Ordering::Relaxed), 1);
    }
}

#[tokio::test]
async fn stream_writer_aborts_after_input_failure() {
    let (writer, aborts) = failing_writer(false, false, false);
    let stream: BlobByteStream = Box::pin(futures_util::stream::once(async {
        Err(CoreError::configuration("input failed"))
    }));
    assert!(put_stream_with_writer(writer, stream).await.is_err());
    assert_eq!(aborts.load(Ordering::Relaxed), 1);
}

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
    let directory = ResourceDirectory::from_path("projects/design/assets").unwrap();

    storage.ensure_directory(&directory).await.unwrap();

    assert!(root.join("projects").is_dir());
    assert!(root.join("projects/design").is_dir());
    assert!(root.join("projects/design/assets").is_dir());
    storage.ensure_directory(&directory).await.unwrap();
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

    let result = storage.put_stream_if_absent(&key, stream).await.unwrap();

    assert_eq!(result.bytes_written(), 16);
    assert_eq!(
        storage.get(&key).await.unwrap(),
        Some(Bytes::from_static(b"large file bytes"))
    );
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
async fn fs_storage_put_stream_if_absent_rejects_existing_blob() {
    let storage = storage("fs-stream-if-absent");
    let key = StorageKey::new("assets/large.bin").unwrap();
    let first: BlobByteStream = Box::pin(futures_util::stream::iter([Ok(Bytes::from_static(
        b"first",
    ))]));
    let second: BlobByteStream = Box::pin(futures_util::stream::iter([Ok(Bytes::from_static(
        b"second",
    ))]));

    storage.put_stream_if_absent(&key, first).await.unwrap();
    let error = storage
        .put_stream_if_absent(&key, second)
        .await
        .unwrap_err();

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

fn storage_with_root(name: &str) -> (OpenDalBlobStorage, PathBuf) {
    let root = unique_temp_path(name);
    std::fs::create_dir_all(&root).unwrap();
    let storage = OpenDalBlobStorage::from_local_config(&LocalBlobConfig {
        root: root.clone(),
        ..LocalBlobConfig::default()
    })
    .unwrap();

    (storage, root)
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
