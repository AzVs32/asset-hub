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
    async fn health_check(&self) -> Result<(), CoreError> {
        self.operator
            .stat("")
            .await
            .map(|_| ())
            .map_err(|error| CoreError::storage("health_check", error))
    }

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

    async fn move_if_absent(&self, from: &StorageKey, to: &StorageKey) -> Result<(), CoreError> {
        if let Some(root) = &self.fs_root {
            let source = root.join(from.as_str());
            let target = root.join(to.as_str());
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| CoreError::storage("move_if_absent.create_parent", error))?;
            }
            std::fs::hard_link(&source, &target).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    CoreError::conflict(format!("storage key `{to}` already exists"))
                } else {
                    CoreError::storage("move_if_absent.link", error)
                }
            })?;
            if let Err(error) = std::fs::remove_file(&source) {
                let _ = std::fs::remove_file(&target);
                return Err(CoreError::storage("move_if_absent.remove_source", error));
            }
            cleanup_empty_fs_parent_dirs(root, from);
            return Ok(());
        }

        self.operator
            .stat(to.as_str())
            .await
            .map(|_| ())
            .or_else(|error| {
                if error.kind() == ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(CoreError::storage("move_if_absent.stat_target", error))
                }
            })?;
        if self.operator.exists(to.as_str()).await.unwrap_or(false) {
            return Err(CoreError::conflict(format!(
                "storage key `{to}` already exists"
            )));
        }
        self.operator
            .rename(from.as_str(), to.as_str())
            .await
            .map_err(|error| CoreError::storage("move_if_absent.rename", error))
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
mod tests;
