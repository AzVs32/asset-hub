use asset_core::CoreError;
use asset_core::domain::{DirectoryPath, StorageKey};
use asset_core::port::{
    BlobByteStream, BlobStorage, DirectoryStorage, RESERVED_BLOB_STORAGE_PREFIX, StagedBlob,
};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use opendal::services::Fs;
use opendal::{ErrorKind, Operator};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// 基于 OpenDAL `Operator` 的对象存储适配器。
///
/// 当前本地后端直接使用文件系统 API，避免 OpenDAL 0.57 的高层路径归一化裁掉对象键
/// 首尾空白。S3 本身允许对象键包含空格，但未来接入时必须同样使用可保留原始键的访问
/// 方式并通过契约测试，不能只替换 `Operator` builder。
#[derive(Clone)]
pub struct OpenDalBlobStorage {
    operator: Operator,
    local_root: Option<PathBuf>,
}

impl OpenDalBlobStorage {
    /// 根据已经归一化的本地根目录创建对象存储适配器。
    pub fn from_local_root(root: &Path) -> Result<Self, CoreError> {
        if root.as_os_str().is_empty() {
            return Err(CoreError::configuration(
                "blob local root must not be empty",
            ));
        }
        let root_text = root.to_string_lossy();
        let builder = Fs::default().root(root_text.as_ref());
        let operator = Operator::new(builder)
            .map_err(|error| CoreError::storage("fs.build", error))?
            .finish();

        Ok(Self {
            operator,
            local_root: Some(root.to_path_buf()),
        })
    }
}

#[async_trait::async_trait]
impl BlobStorage for OpenDalBlobStorage {
    async fn health_check(&self) -> Result<(), CoreError> {
        if let Some(root) = &self.local_root {
            return tokio::fs::metadata(root)
                .await
                .map(|_| ())
                .map_err(|error| CoreError::storage("health_check", error));
        }
        self.operator
            .stat("")
            .await
            .map(|_| ())
            .map_err(|error| CoreError::storage("health_check", error))
    }

    async fn put(&self, key: &StorageKey, data: Bytes) -> Result<(), CoreError> {
        if let Some(root) = &self.local_root {
            let path = root.join(key.as_str());
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| CoreError::storage("put.create_parent", error))?;
            }
            return tokio::fs::write(path, data)
                .await
                .map_err(|error| CoreError::storage("put", error));
        }
        self.operator
            .write(key.as_str(), data)
            .await
            .map(|_| ())
            .map_err(|error| CoreError::storage("put", error))
    }

    async fn stage_stream(&self, data: BlobByteStream) -> Result<StagedBlob, CoreError> {
        let root = self.local_root.as_ref().ok_or_else(|| {
            CoreError::configuration("staged uploads require the local blob storage backend")
        })?;
        let key = StorageKey::new(format!(
            "{RESERVED_BLOB_STORAGE_PREFIX}/uploads/{}",
            uuid::Uuid::now_v7()
        ))?;
        write_local_staged_stream(root.join(key.as_str()), &key, data).await
    }

    async fn publish_staged_if_absent(
        &self,
        staged: &StagedBlob,
        target: &StorageKey,
    ) -> Result<(), CoreError> {
        let root = self.local_root.as_ref().ok_or_else(|| {
            CoreError::configuration("staged uploads require the local blob storage backend")
        })?;
        if !is_upload_staging_key(staged.key()) {
            return Err(CoreError::configuration(format!(
                "staged blob `{}` is outside the upload staging namespace",
                staged.key()
            )));
        }
        let source = root.join(staged.key().as_str());
        let target_path = root.join(target.as_str());
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| CoreError::storage("staged_publish.create_parent", error))?;
        }
        tokio::fs::hard_link(source, target_path)
            .await
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    CoreError::conflict(format!("storage key `{target}` already exists"))
                } else {
                    CoreError::storage("staged_publish.link", error)
                }
            })
    }

    async fn discard_staged(&self, staged: &StagedBlob) -> Result<(), CoreError> {
        if !is_upload_staging_key(staged.key()) {
            return Err(CoreError::configuration(format!(
                "staged blob `{}` is outside the upload staging namespace",
                staged.key()
            )));
        }
        self.delete(staged.key()).await
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, CoreError> {
        if let Some(root) = &self.local_root {
            return match tokio::fs::read(root.join(key.as_str())).await {
                Ok(data) => Ok(Some(Bytes::from(data))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(CoreError::storage("get", error)),
            };
        }
        match self.operator.read(key.as_str()).await {
            Ok(buffer) => Ok(Some(buffer.to_bytes())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CoreError::storage("get", error)),
        }
    }

    async fn get_stream(&self, key: &StorageKey) -> Result<Option<BlobByteStream>, CoreError> {
        if let Some(root) = &self.local_root {
            let file = match tokio::fs::File::open(root.join(key.as_str())).await {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(CoreError::storage("get_stream.open", error)),
            };
            return Ok(Some(local_file_stream(file, None, "get_stream.read")));
        }
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
        if let Some(root) = &self.local_root {
            let length = end
                .checked_sub(start)
                .and_then(|length| length.checked_add(1))
                .ok_or_else(|| CoreError::configuration("invalid blob byte range"))?;
            let mut file = match tokio::fs::File::open(root.join(key.as_str())).await {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(CoreError::storage("get_range_stream.open", error)),
            };
            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|error| CoreError::storage("get_range_stream.seek", error))?;
            return Ok(Some(local_file_stream(
                file,
                Some(length),
                "get_range_stream.read",
            )));
        }
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
        if let Some(root) = &self.local_root {
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
            if is_internal_key(from) {
                cleanup_internal_fs_parents(root, &source)?;
            }
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
        if let Some(root) = &self.local_root {
            let path = root.join(key.as_str());
            match tokio::fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(CoreError::storage("delete", error)),
            }
            if is_internal_key(key) {
                cleanup_internal_fs_parents(root, &path)?;
            }
            return Ok(());
        }
        self.operator
            .delete(key.as_str())
            .await
            .map_err(|error| CoreError::storage("delete", error))?;
        if let Some(root) = &self.local_root
            && is_internal_key(key)
        {
            cleanup_internal_fs_parents(root, &root.join(key.as_str()))?;
        }
        Ok(())
    }
}

async fn write_local_staged_stream(
    path: PathBuf,
    key: &StorageKey,
    mut data: BlobByteStream,
) -> Result<StagedBlob, CoreError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| CoreError::storage("stage_stream.create_parent", error))?;
    }
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .map_err(|error| CoreError::storage("stage_stream.open", error))?;
    let mut staged_file = StagedFile::new(path, file);
    let mut bytes_written = 0_u64;
    while let Some(chunk) = data.next().await {
        let chunk = chunk?;
        bytes_written = bytes_written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| CoreError::storage("stage_stream.size", SizeOverflow))?;
        staged_file
            .file()
            .write_all(&chunk)
            .await
            .map_err(|error| CoreError::storage("stage_stream.write", error))?;
    }
    staged_file
        .file()
        .flush()
        .await
        .map_err(|error| CoreError::storage("stage_stream.flush", error))?;
    staged_file
        .file()
        .sync_all()
        .await
        .map_err(|error| CoreError::storage("stage_stream.sync", error))?;
    staged_file.finish();
    Ok(StagedBlob::new(key.clone(), bytes_written))
}

struct StagedFile {
    path: PathBuf,
    file: Option<tokio::fs::File>,
    complete: bool,
}

impl StagedFile {
    fn new(path: PathBuf, file: tokio::fs::File) -> Self {
        Self {
            path,
            file: Some(file),
            complete: false,
        }
    }

    fn file(&mut self) -> &mut tokio::fs::File {
        self.file.as_mut().expect("staged file must still be open")
    }

    fn finish(mut self) {
        drop(self.file.take());
        self.complete = true;
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        drop(self.file.take());
        if !self.complete {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn local_file_stream(
    file: tokio::fs::File,
    remaining: Option<u64>,
    operation: &'static str,
) -> BlobByteStream {
    Box::pin(futures_util::stream::try_unfold(
        (file, remaining),
        move |(mut file, remaining)| async move {
            if remaining == Some(0) {
                return Ok(None);
            }
            let capacity = remaining
                .map(|remaining| remaining.min(256 * 1024) as usize)
                .unwrap_or(256 * 1024);
            let mut buffer = vec![0_u8; capacity];
            let read = file
                .read(&mut buffer)
                .await
                .map_err(|error| CoreError::storage(operation, error))?;
            if read == 0 {
                return Ok(None);
            }
            buffer.truncate(read);
            let remaining = remaining.map(|remaining| remaining - read as u64);
            Ok(Some((Bytes::from(buffer), (file, remaining))))
        },
    ))
}

#[async_trait::async_trait]
impl DirectoryStorage for OpenDalBlobStorage {
    async fn ensure_directory(&self, directory: &DirectoryPath) -> Result<(), CoreError> {
        let mut path = String::new();

        for name in directory.path().split('/').filter(|name| !name.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(name);
            let current = DirectoryPath::from_path(path.clone())?;

            if let Some(root) = &self.local_root {
                let physical = root.join(current.path());
                match std::fs::metadata(&physical) {
                    Ok(metadata) if metadata.is_dir() => continue,
                    Ok(_) => {
                        return Err(CoreError::conflict(format!(
                            "storage directory `{current}` is occupied by a file"
                        )));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        std::fs::create_dir(&physical)
                            .map_err(|error| CoreError::storage("directory.create", error))?;
                    }
                    Err(error) => return Err(CoreError::storage("directory.metadata", error)),
                }
                continue;
            }

            let marker = format!("{}/", current.path());
            match self.operator.stat(&marker).await {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return Err(CoreError::conflict(format!(
                        "storage directory `{current}` is occupied by an object"
                    )));
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    self.operator
                        .create_dir(&marker)
                        .await
                        .map_err(|error| CoreError::storage("directory.create", error))?;
                }
                Err(error) => return Err(CoreError::storage("directory.stat", error)),
            }
        }

        Ok(())
    }

    async fn move_directory(
        &self,
        from: &DirectoryPath,
        to: &DirectoryPath,
    ) -> Result<(), CoreError> {
        let root = self.local_root.as_ref().ok_or_else(|| {
            CoreError::configuration(
                "directory moves are not implemented for the configured object storage",
            )
        })?;
        let source = root.join(from.path());
        let destination = root.join(to.path());
        if tokio::fs::try_exists(&destination)
            .await
            .map_err(|error| CoreError::storage("directory_move.inspect_target", error))?
        {
            return Err(CoreError::conflict(format!(
                "directory `{}` already exists",
                to.path()
            )));
        }
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| CoreError::storage("directory_move.create_parent", error))?;
        }
        tokio::fs::rename(source, destination)
            .await
            .map_err(|error| CoreError::storage("directory_move", error))
    }
}

fn is_internal_key(key: &StorageKey) -> bool {
    key.as_str() == RESERVED_BLOB_STORAGE_PREFIX
        || key
            .as_str()
            .strip_prefix(RESERVED_BLOB_STORAGE_PREFIX)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_upload_staging_key(key: &StorageKey) -> bool {
    key.as_str()
        .strip_prefix(&format!("{RESERVED_BLOB_STORAGE_PREFIX}/uploads/"))
        .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('/'))
}

/// `.asset-hub` 不属于用户目录模型，可以在内部对象删除后清理其空目录。
/// 用户可见目录绝不在 Blob 操作中隐式删除。
fn cleanup_internal_fs_parents(
    root: &std::path::Path,
    blob: &std::path::Path,
) -> Result<(), CoreError> {
    let internal_root = root.join(RESERVED_BLOB_STORAGE_PREFIX);
    let mut current = blob.parent();
    while let Some(directory) = current {
        if !directory.starts_with(&internal_root) {
            break;
        }
        match std::fs::remove_dir(directory) {
            Ok(()) => current = directory.parent(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                current = directory.parent();
            }
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(error) => return Err(CoreError::storage("internal_directory.cleanup", error)),
        }
    }
    Ok(())
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
