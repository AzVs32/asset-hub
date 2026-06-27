use crate::config::BlobConfig;
use asset_core::CoreError;
use asset_core::domain::StorageKey;
use asset_core::port::{BlobByteStream, BlobStorage, BlobWriteResult};
use bytes::Bytes;
use futures_util::StreamExt;
use opendal::services::Fs;
use opendal::{ErrorKind, Operator};

/// 基于 OpenDAL `Operator` 的对象存储适配器。
///
/// 当前默认使用 Fs backend。后续接入 S3 时，可以复用该适配器结构，只替换 `Operator`
/// 的构建方式。
#[derive(Clone)]
pub struct OpenDalBlobStorage {
    operator: Operator,
}

impl OpenDalBlobStorage {
    /// 使用 OpenDAL `Operator` 创建适配器。
    pub fn new(operator: Operator) -> Self {
        Self { operator }
    }

    /// 根据 Fs 配置创建对象存储适配器。
    pub fn from_config(config: &BlobConfig) -> Result<Self, CoreError> {
        let root = config.fs_root.to_string_lossy();
        let builder = Fs::default().root(root.as_ref());
        let operator = Operator::new(builder)
            .map_err(|error| CoreError::storage("fs.build", error))?
            .finish();

        Ok(Self::new(operator))
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

    async fn put_stream(
        &self,
        key: &StorageKey,
        mut data: BlobByteStream,
    ) -> Result<BlobWriteResult, CoreError> {
        let mut writer = self
            .operator
            .writer(key.as_str())
            .await
            .map_err(|error| CoreError::storage("put_stream.open", error))?;
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
                return Err(CoreError::storage("put_stream.write", error));
            }
        }

        writer
            .close()
            .await
            .map_err(|error| CoreError::storage("put_stream.close", error))?;

        Ok(BlobWriteResult::new(bytes_written))
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, CoreError> {
        match self.operator.read(key.as_str()).await {
            Ok(buffer) => Ok(Some(buffer.to_bytes())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CoreError::storage("get", error)),
        }
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), CoreError> {
        self.operator
            .delete(key.as_str())
            .await
            .map_err(|error| CoreError::storage("delete", error))
    }

    async fn exists(&self, key: &StorageKey) -> Result<bool, CoreError> {
        self.operator
            .exists(key.as_str())
            .await
            .map_err(|error| CoreError::storage("exists", error))
    }
}

#[derive(Debug)]
struct SizeOverflow;

impl std::fmt::Display for SizeOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("stream size exceeds u64::MAX")
    }
}

impl std::error::Error for SizeOverflow {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn fs_storage_roundtrips_blob_content() {
        let storage = storage("fs-roundtrip");
        let key = StorageKey::new("assets/image.png").unwrap();
        let data = Bytes::from_static(b"image bytes");

        storage.put(&key, data.clone()).await.unwrap();

        assert!(storage.exists(&key).await.unwrap());
        assert_eq!(storage.get(&key).await.unwrap(), Some(data));

        storage.delete(&key).await.unwrap();

        assert!(!storage.exists(&key).await.unwrap());
        assert_eq!(storage.get(&key).await.unwrap(), None);
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

        let result = storage.put_stream(&key, stream).await.unwrap();

        assert_eq!(result.bytes_written(), 16);
        assert_eq!(
            storage.get(&key).await.unwrap(),
            Some(Bytes::from_static(b"large file bytes"))
        );
    }

    fn storage(name: &str) -> OpenDalBlobStorage {
        let root = unique_temp_path(name);
        std::fs::create_dir_all(&root).unwrap();

        OpenDalBlobStorage::from_config(&BlobConfig { fs_root: root }).unwrap()
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
    }
}
