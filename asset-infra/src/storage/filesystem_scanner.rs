use asset_core::CoreError;
use asset_core::domain::{ResourceDirectory, StorageKey};
use asset_core::port::{
    RESERVED_BLOB_STORAGE_PREFIX, ScannedBlob, ScannedStorageEntry, StoragePrefix,
    StorageScanStream, StorageScanner,
};
use std::path::{Component, Path, PathBuf};
use tokio::sync::mpsc;

const SCAN_STREAM_BUFFER_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct FileSystemScanner {
    root: PathBuf,
}

impl FileSystemScanner {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait::async_trait]
impl StorageScanner for FileSystemScanner {
    fn scan(&self, prefix: &StoragePrefix) -> StorageScanStream {
        let root = self.root.clone();
        let prefix = prefix.clone();
        let (sender, receiver) = mpsc::channel(SCAN_STREAM_BUFFER_CAPACITY);
        let failure_sender = sender.clone();
        let producer =
            tokio::task::spawn_blocking(move || produce_storage_entries(&root, &prefix, sender));
        tokio::spawn(async move {
            if let Err(error) = producer.await {
                let _ = failure_sender
                    .send(Err(CoreError::configuration(format!(
                        "scan task failed: {error}"
                    ))))
                    .await;
            }
        });
        Box::pin(futures_util::stream::unfold(
            receiver,
            |mut receiver| async move { receiver.recv().await.map(|entry| (entry, receiver)) },
        ))
    }

    async fn inspect(&self, key: &StorageKey) -> Result<Option<ScannedBlob>, CoreError> {
        let root = self.root.clone();
        let key = key.clone();
        tokio::task::spawn_blocking(move || inspect_file(&root, &key))
            .await
            .map_err(|error| CoreError::configuration(format!("inspect task failed: {error}")))?
    }
}

fn inspect_file(root: &Path, key: &StorageKey) -> Result<Option<ScannedBlob>, CoreError> {
    if key.as_str() == RESERVED_BLOB_STORAGE_PREFIX
        || key
            .as_str()
            .starts_with(&format!("{RESERVED_BLOB_STORAGE_PREFIX}/"))
    {
        return Ok(None);
    }

    let path = root.join(key.as_str());
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CoreError::storage("inspect.metadata", error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(None);
    }

    Ok(Some(ScannedBlob {
        key: key.clone(),
        size: metadata.len(),
        mime_type: content_type_from_path(&path).map(str::to_owned),
    }))
}

fn produce_storage_entries(
    root: &Path,
    prefix: &StoragePrefix,
    sender: mpsc::Sender<Result<ScannedStorageEntry, CoreError>>,
) {
    let mut emit = |entry| sender.blocking_send(Ok(entry)).is_ok();
    if let Err(error) = visit_storage_entries(root, prefix, &mut emit) {
        let _ = sender.blocking_send(Err(error));
    }
}

fn visit_storage_entries(
    root: &Path,
    prefix: &StoragePrefix,
    emit: &mut impl FnMut(ScannedStorageEntry) -> bool,
) -> Result<(), CoreError> {
    if prefix_in_reserved_namespace(prefix) {
        return Ok(());
    }

    let root = root.canonicalize().map_err(|error| {
        CoreError::configuration(format!("storage root is not readable: {error}"))
    })?;
    let scan_root = root
        .join(prefix.as_str())
        .canonicalize()
        .map_err(|error| CoreError::configuration(format!("scan path is not readable: {error}")))?;
    if !scan_root.starts_with(&root) || !scan_root.is_dir() {
        return Err(CoreError::configuration(
            "scan path must be a directory inside storage root",
        ));
    }

    if !prefix.is_root()
        && !emit(ScannedStorageEntry::Directory(
            ResourceDirectory::from_path(prefix.as_str())?,
        ))
    {
        return Ok(());
    }
    visit_directory(&root, &scan_root, emit)?;
    Ok(())
}

fn visit_directory(
    root: &Path,
    current: &Path,
    emit: &mut impl FnMut(ScannedStorageEntry) -> bool,
) -> Result<bool, CoreError> {
    for entry in
        std::fs::read_dir(current).map_err(|error| CoreError::storage("scan.read_dir", error))?
    {
        let entry = entry.map_err(|error| CoreError::storage("scan.read_dir_entry", error))?;
        if current == root && entry.file_name().to_str() == Some(RESERVED_BLOB_STORAGE_PREFIX) {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| CoreError::storage("scan.metadata", error))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let directory = ResourceDirectory::from_path(relative_storage_path(root, &path)?)?;
            if !emit(ScannedStorageEntry::Directory(directory)) {
                return Ok(false);
            }
            if !visit_directory(root, &path, emit)? {
                return Ok(false);
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let blob = ScannedBlob {
            key: StorageKey::new(relative_storage_path(root, &path)?)?,
            size: metadata.len(),
            mime_type: content_type_from_path(&path).map(str::to_owned),
        };
        if !emit(ScannedStorageEntry::Blob(blob)) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn relative_storage_path(root: &Path, path: &Path) -> Result<String, CoreError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| CoreError::configuration("scanned path escaped storage root"))?;
    relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(
                part.to_str()
                    .ok_or_else(|| CoreError::configuration("storage path must be valid UTF-8")),
            ),
            Component::CurDir => None,
            _ => Some(Err(CoreError::configuration("invalid storage path"))),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}

fn prefix_in_reserved_namespace(prefix: &StoragePrefix) -> bool {
    prefix.as_str() == RESERVED_BLOB_STORAGE_PREFIX
        || prefix
            .as_str()
            .starts_with(&format!("{RESERVED_BLOB_STORAGE_PREFIX}/"))
}

fn content_type_from_path(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "txt" => Some("text/plain; charset=utf-8"),
        "md" | "markdown" => Some("text/markdown; charset=utf-8"),
        "json" => Some("application/json"),
        "html" | "htm" => Some("text/html; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "js" | "mjs" => Some("text/javascript"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "pdf" => Some("application/pdf"),
        "epub" => Some("application/epub+zip"),
        "mp3" => Some("audio/mpeg"),
        "mp4" => Some("video/mp4"),
        "zip" => Some("application/zip"),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
