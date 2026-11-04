//! Storage behavior shared by independent driver test suites.

use crate::{
    application::port::{Storage, StorageFileReader},
    domain::storage::{
        DEFAULT_DIR_PAGE_LIMIT, MAX_DIR_PAGE_LIMIT, StorageCreateDirRequest, StorageDirQuery,
        StorageEntryKind, StorageErrorKind, StorageMoveEntryRequest, StoragePath,
        StorageReadFileRequest, StorageStatEntryRequest, StorageWriteFileRequest,
    },
};
use std::num::NonZeroUsize;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn path(value: &str) -> TestResult<StoragePath> {
    Ok(StoragePath::parse(value)?)
}

async fn put(storage: &dyn Storage, name: &str, bytes: &[u8]) -> TestResult {
    let mut writer = storage
        .write_file(StorageWriteFileRequest::new(path(name)?))
        .await?;
    writer.write_chunk(bytes).await?;
    writer.commit().await?;
    Ok(())
}

async fn collect(reader: &mut dyn StorageFileReader) -> TestResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        assert!(!chunk.is_empty());
        bytes.extend(chunk);
    }
    assert!(reader.next_chunk().await?.is_none());
    Ok(bytes)
}

async fn get(storage: &dyn Storage, name: &str) -> TestResult<Vec<u8>> {
    collect(
        storage
            .read_file(StorageReadFileRequest::new(path(name)?))
            .await?
            .as_mut(),
    )
    .await
}

async fn mkdir(storage: &dyn Storage, name: &str) -> TestResult {
    storage
        .create_dir(StorageCreateDirRequest {
            path: path(name)?,
            recursive: true,
        })
        .await?;
    Ok(())
}

async fn exists(storage: &dyn Storage, name: &str) -> TestResult<bool> {
    match storage
        .stat_entry(StorageStatEntryRequest { path: path(name)? })
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == StorageErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Assert range reads, EOF and invalid-target semantics on fresh range-readable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_range_read_contract(storage: &dyn Storage) -> TestResult {
    let bytes: Vec<u8> = (0..150_000).map(|index| (index % 251) as u8).collect();
    put(storage, "file", &bytes).await?;
    put(storage, "empty", b"").await?;
    mkdir(storage, "dir").await?;

    for name in ["file", "empty"] {
        let content = if name == "file" {
            bytes.as_slice()
        } else {
            &[]
        };
        let eof = content.len() as u64;
        for (offset, length, expected) in [
            (u64::MAX, None, &[][..]),
            (u64::MAX, Some(0), &[][..]),
            (u64::MAX, Some(1), &[][..]),
            (u64::MAX, Some(u64::MAX), &[][..]),
            (i64::MAX as u64 + 1, None, &[][..]),
            (eof, None, &[][..]),
            (eof + 1, None, &[][..]),
            (0, Some(0), &[][..]),
            (0, None, content),
            (7, Some(11), content.get(7..18).unwrap_or(&[])),
            (7, Some(u64::MAX), content.get(7..).unwrap_or(&[])),
        ] {
            let mut reader = storage
                .read_file(StorageReadFileRequest {
                    path: path(name)?,
                    offset,
                    length,
                })
                .await?;
            let mut actual = Vec::new();
            while let Some(chunk) = reader.next_chunk().await? {
                assert!(!chunk.is_empty());
                assert!(chunk.len() <= storage.chunk_size().get());
                actual.extend(chunk);
            }
            assert!(reader.next_chunk().await?.is_none());
            assert!(reader.next_chunk().await?.is_none());
            assert_eq!(
                actual, expected,
                "{name}: offset={offset}, length={length:?}"
            );
        }
    }

    // Empty ranges still validate the target instead of hiding missing files or dirs.
    for (name, expected) in [
        ("missing", StorageErrorKind::NotFound),
        ("dir", StorageErrorKind::IsDir),
    ] {
        for length in [None, Some(0)] {
            let error = storage
                .read_file(StorageReadFileRequest {
                    path: path(name)?,
                    offset: u64::MAX,
                    length,
                })
                .await
                .err()
                .ok_or("expected invalid read target")?;
            assert_eq!(error.kind(), expected);
        }
    }
    Ok(())
}

/// Assert recursive and exclusive directory creation on fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_create_dir_contract(storage: &dyn Storage) -> TestResult {
    put(storage, "file", b"retained file").await?;
    mkdir(storage, "dir").await?;
    put(storage, "dir/child", b"retained child").await?;

    for (name, recursive, expected) in [
        ("file", true, StorageErrorKind::NotDir),
        ("file/child", true, StorageErrorKind::NotDir),
        ("file/child", false, StorageErrorKind::NotDir),
        ("file", false, StorageErrorKind::AlreadyExists),
        ("dir", false, StorageErrorKind::AlreadyExists),
        ("missing/child", false, StorageErrorKind::NotFound),
    ] {
        let error = storage
            .create_dir(StorageCreateDirRequest {
                path: path(name)?,
                recursive,
            })
            .await
            .err()
            .ok_or("expected create_dir failure")?;
        assert_eq!(error.kind(), expected, "{name}: recursive={recursive}");
    }
    assert!(!exists(storage, "missing").await?);

    for name in ["", "dir"] {
        let before = storage
            .stat_entry(StorageStatEntryRequest { path: path(name)? })
            .await?;
        let entry = storage
            .create_dir(StorageCreateDirRequest {
                path: path(name)?,
                recursive: true,
            })
            .await?;
        assert_eq!(entry.path, before.path);
        assert_eq!(entry.kind, StorageEntryKind::Dir);
        assert_eq!(entry.size, before.size);
        assert_eq!(entry.modified, before.modified);
        let after = storage
            .stat_entry(StorageStatEntryRequest { path: path(name)? })
            .await?;
        assert_eq!(after.kind, before.kind);
        assert_eq!(after.modified, before.modified);

        let error = storage
            .create_dir(StorageCreateDirRequest {
                path: path(name)?,
                recursive: false,
            })
            .await
            .err()
            .ok_or("expected existing directory")?;
        assert_eq!(error.kind(), StorageErrorKind::AlreadyExists);
    }
    assert_eq!(get(storage, "file").await?, b"retained file");
    assert_eq!(get(storage, "dir/child").await?, b"retained child");

    let entry = storage
        .create_dir(StorageCreateDirRequest {
            path: path("new/parents/leaf")?,
            recursive: true,
        })
        .await?;
    assert_eq!(entry.path, path("new/parents/leaf")?);
    assert_eq!(entry.kind, StorageEntryKind::Dir);
    for name in ["new", "new/parents", "new/parents/leaf"] {
        assert_eq!(
            storage
                .stat_entry(StorageStatEntryRequest { path: path(name)? })
                .await?
                .kind,
            StorageEntryKind::Dir
        );
    }
    let entry = storage
        .create_dir(StorageCreateDirRequest {
            path: path("new/sibling")?,
            recursive: false,
        })
        .await?;
    assert_eq!(entry.kind, StorageEntryKind::Dir);
    Ok(())
}

/// Assert directory cursor validation and ordered pagination on fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_pagination_contract(storage: &dyn Storage) -> TestResult {
    let oversized = NonZeroUsize::new(MAX_DIR_PAGE_LIMIT.get() + 1).ok_or("zero limit")?;
    for name in ["", "missing"] {
        let query = StorageDirQuery {
            path: path(name)?,
            after: Some("../invalid".into()),
            limit: oversized,
        };
        let error = storage
            .list_entries(query)
            .await
            .err()
            .ok_or("expected oversized page")?;
        assert_eq!(error.kind(), StorageErrorKind::InvalidRequest);
        for after in ["", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
            let query = StorageDirQuery {
                path: path(name)?,
                after: Some(after.into()),
                limit: DEFAULT_DIR_PAGE_LIMIT,
            };
            let error = storage
                .list_entries(query)
                .await
                .err()
                .ok_or("expected invalid cursor")?;
            assert_eq!(error.kind(), StorageErrorKind::InvalidPath);
            assert!(std::error::Error::source(&error).is_some());
        }
    }
    let query = StorageDirQuery {
        path: path(&"x".repeat(StoragePath::MAX_LEN))?,
        after: Some("child".into()),
        limit: MAX_DIR_PAGE_LIMIT,
    };
    let error = storage
        .list_entries(query)
        .await
        .err()
        .ok_or("expected path overflow")?;
    assert_eq!(error.kind(), StorageErrorKind::InvalidPath);

    for name in ["a", "b", "文件"] {
        put(storage, name, b"").await?;
    }
    let page = storage
        .list_entries(StorageDirQuery {
            path: path("")?,
            after: None,
            limit: NonZeroUsize::MIN,
        })
        .await?;
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].path, path("a")?);
    assert_eq!(page.next_after.as_deref(), Some("a"));
    let page = storage
        .list_entries(StorageDirQuery {
            path: path("")?,
            after: page.next_after,
            limit: MAX_DIR_PAGE_LIMIT,
        })
        .await?;
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["b", "文件"]
    );
    assert!(page.next_after.is_none());
    Ok(())
}

/// Assert same-path rename semantics for the modes advertised by fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_same_path_rename_contract(storage: &dyn Storage) -> TestResult {
    put(storage, "file", b"file content").await?;
    mkdir(storage, "dir").await?;
    put(storage, "dir/child", b"child content").await?;

    for replace in [false, true] {
        let support = storage.capabilities().rename();
        let supported = if replace {
            support.replace
        } else {
            support.no_replace
        };
        if !supported {
            continue;
        }
        for name in ["file", "dir"] {
            let before = storage
                .stat_entry(StorageStatEntryRequest { path: path(name)? })
                .await?;
            let result = storage
                .move_entry(StorageMoveEntryRequest {
                    source: path(name)?,
                    destination: path(name)?,
                    replace,
                })
                .await;
            if replace {
                let entry = result?;
                assert_eq!(entry.path, before.path);
                assert_eq!(entry.kind, before.kind);
                assert_eq!(entry.size, before.size);
                assert_eq!(entry.modified, before.modified);
            } else {
                assert_eq!(
                    result.err().ok_or("expected existing destination")?.kind(),
                    StorageErrorKind::AlreadyExists
                );
            }
            let after = storage
                .stat_entry(StorageStatEntryRequest { path: path(name)? })
                .await?;
            assert_eq!(after.path, before.path);
            assert_eq!(after.kind, before.kind);
            assert_eq!(after.size, before.size);
            assert_eq!(after.modified, before.modified);
            assert_eq!(get(storage, "file").await?, b"file content");
            assert_eq!(get(storage, "dir/child").await?, b"child content");
        }

        let error = storage
            .move_entry(StorageMoveEntryRequest {
                source: path("missing")?,
                destination: path("missing")?,
                replace,
            })
            .await
            .err()
            .ok_or("expected missing source")?;
        assert_eq!(error.kind(), StorageErrorKind::NotFound);
        assert!(!exists(storage, "missing").await?);

        let error = storage
            .move_entry(StorageMoveEntryRequest {
                source: path("")?,
                destination: path("")?,
                replace,
            })
            .await
            .err()
            .ok_or("expected protected root")?;
        assert_eq!(error.kind(), StorageErrorKind::RootProtected);
    }
    Ok(())
}
