use super::*;
use futures_util::StreamExt;

#[test]
fn scan_skips_reserved_asset_hub_directory() {
    let root = unique_temp_path("scanner-reserved");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::create_dir_all(
        root.join(RESERVED_BLOB_STORAGE_PREFIX)
            .join("action-effects/action-replacements"),
    )
    .unwrap();
    std::fs::write(root.join("docs/readme.md"), b"# Readme").unwrap();
    std::fs::write(
        root.join(RESERVED_BLOB_STORAGE_PREFIX)
            .join("action-effects/action-replacements/temp"),
        b"scratch",
    )
    .unwrap();

    let entries = scanned_entries(&root, &StoragePrefix::root());
    let files = entries
        .iter()
        .filter_map(|entry| match entry {
            ScannedStorageEntry::Blob(file) => Some(file),
            ScannedStorageEntry::Directory(_) => None,
        })
        .collect::<Vec<_>>();
    let mut directories = entries
        .iter()
        .filter_map(|entry| match entry {
            ScannedStorageEntry::Directory(directory) => Some(directory.path()),
            ScannedStorageEntry::Blob(_) => None,
        })
        .collect::<Vec<_>>();
    directories.sort_unstable();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].key.as_str(), "docs/readme.md");
    assert_eq!(directories, vec!["docs"]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn scan_includes_manually_created_empty_directories() {
    let root = unique_temp_path("scanner-empty-directories");
    std::fs::create_dir_all(root.join("manual/empty/nested")).unwrap();

    let entries = scanned_entries(&root, &StoragePrefix::root());
    let mut directories = entries
        .iter()
        .filter_map(|entry| match entry {
            ScannedStorageEntry::Directory(directory) => Some(directory.path()),
            ScannedStorageEntry::Blob(_) => None,
        })
        .collect::<Vec<_>>();
    directories.sort_unstable();

    assert_eq!(
        directories,
        vec!["manual", "manual/empty", "manual/empty/nested"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn scan_reserved_directory_returns_no_files() {
    let root = unique_temp_path("scanner-reserved-direct");
    std::fs::create_dir_all(
        root.join(RESERVED_BLOB_STORAGE_PREFIX)
            .join("action-effects/action-replacements"),
    )
    .unwrap();
    std::fs::write(
        root.join(RESERVED_BLOB_STORAGE_PREFIX)
            .join("action-effects/action-replacements/temp"),
        b"scratch",
    )
    .unwrap();

    let entries = scanned_entries(
        &root,
        &StoragePrefix::new(RESERVED_BLOB_STORAGE_PREFIX).unwrap(),
    );

    assert!(entries.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn scan_streams_more_entries_than_its_internal_buffer() {
    let root = unique_temp_path("scanner-stream-buffer");
    std::fs::create_dir_all(&root).unwrap();
    for index in 0..(SCAN_STREAM_BUFFER_CAPACITY + 32) {
        std::fs::write(root.join(format!("{index:04}.txt")), b"").unwrap();
    }

    let scanner = FileSystemScanner::new(root.clone());
    let mut entries = scanner.scan(&StoragePrefix::root());
    let mut files = 0;
    while let Some(entry) = entries.next().await {
        if matches!(entry.unwrap(), ScannedStorageEntry::Blob(_)) {
            files += 1;
        }
    }

    assert_eq!(files, SCAN_STREAM_BUFFER_CAPACITY + 32);
    std::fs::remove_dir_all(root).unwrap();
}

fn scanned_entries(root: &Path, prefix: &StoragePrefix) -> Vec<ScannedStorageEntry> {
    let mut entries = Vec::new();
    visit_storage_entries(root, prefix, &mut |entry| {
        entries.push(entry);
        true
    })
    .unwrap();
    entries
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
