use asset_core::CoreError;
use asset_core::domain::{ResourceDirectory, StorageKey};
use asset_core::port::{ScannedBlob, StorageScanner};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

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
    async fn scan(
        &self,
        directory: &ResourceDirectory,
        include_sha256: bool,
        max_entries: usize,
    ) -> Result<Vec<ScannedBlob>, CoreError> {
        let root = self.root.clone();
        let directory = directory.clone();
        tokio::task::spawn_blocking(move || {
            scan_files(&root, &directory, include_sha256, max_entries)
        })
        .await
        .map_err(|error| CoreError::configuration(format!("scan task failed: {error}")))?
    }
}

fn scan_files(
    root: &Path,
    directory: &ResourceDirectory,
    include_sha256: bool,
    max_entries: usize,
) -> Result<Vec<ScannedBlob>, CoreError> {
    let root = root.canonicalize().map_err(|error| {
        CoreError::configuration(format!("storage root is not readable: {error}"))
    })?;
    let scan_root = root
        .join(directory.path())
        .canonicalize()
        .map_err(|error| CoreError::configuration(format!("scan path is not readable: {error}")))?;
    if !scan_root.starts_with(&root) || !scan_root.is_dir() {
        return Err(CoreError::configuration(
            "scan path must be a directory inside storage root",
        ));
    }
    let mut files = Vec::new();
    let mut visited = 0;
    collect_files(
        &root,
        &scan_root,
        include_sha256,
        max_entries,
        &mut visited,
        &mut files,
    )?;
    files.sort_by(|left, right| left.key.as_str().cmp(right.key.as_str()));
    Ok(files)
}

fn collect_files(
    root: &Path,
    current: &Path,
    include_sha256: bool,
    max_entries: usize,
    visited: &mut usize,
    files: &mut Vec<ScannedBlob>,
) -> Result<(), CoreError> {
    for entry in
        std::fs::read_dir(current).map_err(|error| CoreError::storage("scan.read_dir", error))?
    {
        *visited += 1;
        if *visited > max_entries {
            return Err(CoreError::configuration(format!(
                "storage scan exceeds the limit of {max_entries} entries"
            )));
        }
        let entry = entry.map_err(|error| CoreError::storage("scan.read_dir_entry", error))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| CoreError::storage("scan.metadata", error))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_files(root, &path, include_sha256, max_entries, visited, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CoreError::configuration("scanned object path escaped storage root"))?;
        let mut parts = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => {
                    parts.push(part.to_str().ok_or_else(|| {
                        CoreError::configuration("storage path must be valid UTF-8")
                    })?)
                }
                Component::CurDir => {}
                _ => return Err(CoreError::configuration("invalid storage object path")),
            }
        }
        files.push(ScannedBlob {
            key: StorageKey::new(parts.join("/"))?,
            size: metadata.len(),
            mime_type: content_type_from_path(&path).map(str::to_owned),
            sha256: include_sha256.then(|| sha256_file(&path)).transpose()?,
        });
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, CoreError> {
    let mut file =
        std::fs::File::open(path).map_err(|error| CoreError::storage("scan.open", error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| CoreError::storage("scan.read", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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
