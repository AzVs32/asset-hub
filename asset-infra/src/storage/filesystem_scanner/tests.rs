use super::*;

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

    let files = scan_files(&root, &StoragePrefix::root(), false, 100).unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].key.as_str(), "docs/readme.md");
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

    let files = scan_files(
        &root,
        &StoragePrefix::new(RESERVED_BLOB_STORAGE_PREFIX).unwrap(),
        false,
        100,
    )
    .unwrap();

    assert!(files.is_empty());
}

fn unique_temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("asset-hub-{name}-{}", uuid::Uuid::now_v7()))
}
