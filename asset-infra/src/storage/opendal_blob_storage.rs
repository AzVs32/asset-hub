use crate::config::BlobConfig;
use asset_core::CoreError;
use asset_core::domain::StorageKey;
use asset_core::port::{BlobByteStream, BlobStorage, BlobWriteResult};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use opendal::services::Fs;
use opendal::{ErrorKind, Operator};
use std::path::{Path, PathBuf};

/// 基于 OpenDAL `Operator` 的对象存储适配器。
///
/// 当前默认使用 Fs backend。后续接入 S3 时，可以复用该适配器结构，只替换 `Operator`
/// 的构建方式。
#[derive(Clone)]
pub struct OpenDalBlobStorage {
    operator: Operator,
    fs_root: Option<PathBuf>,
}

impl OpenDalBlobStorage {
    /// 使用 OpenDAL `Operator` 创建适配器。
    pub fn new(operator: Operator) -> Self {
        Self {
            operator,
            fs_root: None,
        }
    }

    /// 根据 Fs 配置创建对象存储适配器。
    pub fn from_config(config: &BlobConfig) -> Result<Self, CoreError> {
        let root = config.fs_root.to_string_lossy();
        let builder = Fs::default().root(root.as_ref());
        let operator = Operator::new(builder)
            .map_err(|error| CoreError::storage("fs.build", error))?
            .finish();

        Ok(Self {
            operator,
            fs_root: Some(config.fs_root.clone()),
        })
    }

    /// 返回内部 OpenDAL `Operator`。
    pub fn operator(&self) -> &Operator {
        &self.operator
    }
}

#[async_trait::async_trait]
impl BlobStorage for OpenDalBlobStorage {
    async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
        self.operator
            .write(key.as_str(), data)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::storage("put", error))
    }

    async fn put_stream_if_absent(
        &self,
        key: &StorageKey,
        data: BlobByteStream,
    ) -> Result<BlobWriteResult, CoreError> {
        put_stream_with_writer(
            self.operator
                .writer_with(key.as_str())
                .if_not_exists(true)
                .await
                .map_err(|error| {
                    conditional_write_error("put_stream_if_absent.open", key, error)
                })?,
            data,
        )
        .await
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, CoreError> {
        match self.operator.read(key.as_str()).await {
            Ok(buffer) => Ok(Some(buffer.to_bytes())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CoreError::storage("get", error)),
        }
    }

    async fn get_stream(&self, key: &StorageKey) -> Result<Option<BlobByteStream>, CoreError> {
        let reader = match self
            .operator
            .reader_with(key.as_str())
            .chunk(256 * 1024)
            .await
        {
            Ok(reader) => reader,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CoreError::storage("get_stream.open", error)),
        };
        let stream = reader
            .into_bytes_stream(..)
            .await
            .map_err(|error| CoreError::storage("get_stream.open", error))?
            .map_err(|error| CoreError::storage("get_stream.read", error));

        Ok(Some(Box::pin(stream)))
    }

    async fn get_range_stream(
        &self,
        key: &StorageKey,
        start: u64,
        end: u64,
    ) -> Result<Option<BlobByteStream>, CoreError> {
        let reader = match self
            .operator
            .reader_with(key.as_str())
            .chunk(256 * 1024)
            .await
        {
            Ok(reader) => reader,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(CoreError::storage("get_range_stream.open", error)),
        };
        let stream = reader
            .into_bytes_stream(start..end + 1)
            .await
            .map_err(|error| CoreError::storage("get_range_stream.open", error))?
            .map_err(|error| CoreError::storage("get_range_stream.read", error));

        Ok(Some(Box::pin(stream)))
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), CoreError> {
        self.operator
            .delete(key.as_str())
            .await
            .map_err(|error| CoreError::storage("delete", error))?;
        if let Some(root) = &self.fs_root {
            cleanup_empty_fs_parent_dirs(root, key);
        }
        Ok(())
    }
}

fn cleanup_empty_fs_parent_dirs(root: &Path, key: &StorageKey) {
    let mut parts = key.as_str().split('/').collect::<Vec<_>>();
    if parts.len() < 2 || parts.iter().any(|part| part.is_empty() || *part == ".") {
        return;
    }
    parts.pop();

    let mut current = root.to_path_buf();
    for part in parts {
        current.push(part);
    }

    while current != root {
        match std::fs::remove_dir(&current) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                break;
            }
            Err(_) => break,
        }

        if !current.pop() {
            break;
        }
    }
}

async fn put_stream_with_writer(
    mut writer: impl StreamWriter,
    mut data: BlobByteStream,
) -> Result<BlobWriteResult, CoreError> {
    let mut bytes_written = 0_u64;

    while let Some(chunk) = data.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = writer.abort().await;
                return Err(error);
            }
        };

        bytes_written = bytes_written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| CoreError::storage("put_stream.size", SizeOverflow))?;

        if let Err(error) = writer.write(chunk).await {
            let _ = writer.abort().await;
            return Err(CoreError::storage("put_stream.write", WriterFailure(error)));
        }
    }

    if let Err(error) = writer.close().await {
        let _ = writer.abort().await;
        return Err(CoreError::storage("put_stream.close", WriterFailure(error)));
    }

    Ok(BlobWriteResult::new(bytes_written))
}

type WriterError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug)]
struct WriterFailure(WriterError);
impl std::fmt::Display for WriterFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}
impl std::error::Error for WriterFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

#[async_trait::async_trait]
trait StreamWriter: Send {
    async fn write(&mut self, chunk: Bytes) -> Result<(), WriterError>;
    async fn close(&mut self) -> Result<(), WriterError>;
    async fn abort(&mut self) -> Result<(), WriterError>;
}

#[async_trait::async_trait]
impl StreamWriter for opendal::Writer {
    async fn write(&mut self, chunk: Bytes) -> Result<(), WriterError> {
        opendal::Writer::write(self, chunk)
            .await
            .map_err(|error| Box::new(error) as WriterError)
    }

    async fn close(&mut self) -> Result<(), WriterError> {
        opendal::Writer::close(self)
            .await
            .map(|_| ())
            .map_err(|error| Box::new(error) as WriterError)
    }

    async fn abort(&mut self) -> Result<(), WriterError> {
        opendal::Writer::abort(self)
            .await
            .map_err(|error| Box::new(error) as WriterError)
    }
}

fn conditional_write_error(
    operation: &'static str,
    key: &StorageKey,
    error: opendal::Error,
) -> CoreError {
    if matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::ConditionNotMatch
    ) {
        return CoreError::conflict(format!("storage key `{key}` already exists"));
    }

    CoreError::storage(operation, error)
}

#[derive(Debug)]
struct SizeOverflow;

impl std::fmt::Display for SizeOverflow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("stream size exceeds u64")
    }
}

impl std::error::Error for SizeOverflow {}

#[cfg(test)]
mod tests {
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
        let second: BlobByteStream = Box::pin(futures_util::stream::iter([Ok(
            Bytes::from_static(b"second"),
        )]));

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

    fn storage(name: &str) -> OpenDalBlobStorage {
        storage_with_root(name).0
    }

    fn storage_with_root(name: &str) -> (OpenDalBlobStorage, PathBuf) {
        let root = unique_temp_path(name);
        std::fs::create_dir_all(&root).unwrap();
        let storage = OpenDalBlobStorage::from_config(&BlobConfig {
            fs_root: root.clone(),
        })
        .unwrap();

        (storage, root)
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
    }
}
