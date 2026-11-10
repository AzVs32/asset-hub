//! Storage behavior shared by independent driver test suites.

use crate::application::port::{Storage, StorageFileReader};
use crate::domain::storage::{
    MAX_DIR_PAGE_LIMIT, StorageCreateDirRequest, StorageDirQuery, StorageMoveEntryRequest,
    StorageWriteFileRequest,
};
use ah_vfs_storage::RenameSupport;
use ah_vfs_storage::{EntryType, StoragePath};
use ah_vfs_storage::{Failure, Rejection};
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
            .read_file(
                ah_vfs_storage::ReadRequest::new(
                    (path(name)?).to_rooted_expr(),
                    ah_vfs_storage::ReadRange::FULL,
                ),
                Default::default(),
            )
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
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                (path(name)?).to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound) => Ok(false),
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
            (i64::MAX as u64 + 1, None, &[][..]),
            (eof, None, &[][..]),
            (eof + 1, None, &[][..]),
            (0, Some(0), &[][..]),
            (0, None, content),
            (7, Some(11), content.get(7..18).unwrap_or(&[])),
            (7, Some(u64::MAX - 7), content.get(7..).unwrap_or(&[])),
        ] {
            let mut reader = storage
                .read_file(
                    ah_vfs_storage::ReadRequest::new(
                        (path(name)?).to_rooted_expr(),
                        ah_vfs_storage::ReadRange::new(offset, length)?,
                    ),
                    Default::default(),
                )
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
        ("missing", Failure::Rejected(Rejection::NotFound)),
        ("dir", Failure::Rejected(Rejection::IsDirectory)),
    ] {
        for length in [None, Some(0)] {
            let error = storage
                .read_file(
                    ah_vfs_storage::ReadRequest::new(
                        path(name)?.to_rooted_expr(),
                        ah_vfs_storage::ReadRange::new(u64::MAX, length)?,
                    ),
                    Default::default(),
                )
                .await
                .err()
                .ok_or("expected invalid read target")?;
            assert_eq!(error.failure(), expected);
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
        ("file", true, Failure::Rejected(Rejection::NotDirectory)),
        (
            "file/child",
            true,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        (
            "file/child",
            false,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        ("file", false, Failure::Rejected(Rejection::AlreadyExists)),
        ("dir", false, Failure::Rejected(Rejection::AlreadyExists)),
        (
            "missing/child",
            false,
            Failure::Rejected(Rejection::NotFound),
        ),
    ] {
        let error = storage
            .create_dir(StorageCreateDirRequest {
                path: path(name)?,
                recursive,
            })
            .await
            .err()
            .ok_or("expected create_dir failure")?;
        assert_eq!(error.failure(), expected, "{name}: recursive={recursive}");
    }
    assert!(!exists(storage, "missing").await?);

    for name in ["", "dir"] {
        let before = storage
            .stat_entry(
                ah_vfs_storage::StatRequest::new(
                    (path(name)?).to_rooted_expr(),
                    ah_vfs_storage::SymlinkAction::Preserve,
                ),
                Default::default(),
            )
            .await?;
        let entry = storage
            .create_dir(StorageCreateDirRequest {
                path: path(name)?,
                recursive: true,
            })
            .await?;
        assert_eq!(entry.path(), before.path());
        assert_eq!(entry.entry_type(), EntryType::Directory);
        assert_eq!(entry.modified(), before.modified());
        let after = storage
            .stat_entry(
                ah_vfs_storage::StatRequest::new(
                    (path(name)?).to_rooted_expr(),
                    ah_vfs_storage::SymlinkAction::Preserve,
                ),
                Default::default(),
            )
            .await?;
        assert_eq!(after.entry_type(), before.entry_type());
        assert_eq!(after.modified(), before.modified());

        let error = storage
            .create_dir(StorageCreateDirRequest {
                path: path(name)?,
                recursive: false,
            })
            .await
            .err()
            .ok_or("expected existing directory")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::AlreadyExists));
    }
    assert_eq!(get(storage, "file").await?, b"retained file");
    assert_eq!(get(storage, "dir/child").await?, b"retained child");

    let entry = storage
        .create_dir(StorageCreateDirRequest {
            path: path("new/parents/leaf")?,
            recursive: true,
        })
        .await?;
    assert_eq!(entry.path(), &path("new/parents/leaf")?);
    assert_eq!(entry.entry_type(), EntryType::Directory);
    for name in ["new", "new/parents", "new/parents/leaf"] {
        assert_eq!(
            storage
                .stat_entry(
                    ah_vfs_storage::StatRequest::new(
                        (path(name)?).to_rooted_expr(),
                        ah_vfs_storage::SymlinkAction::Preserve
                    ),
                    Default::default()
                )
                .await?
                .entry_type(),
            EntryType::Directory
        );
    }
    let entry = storage
        .create_dir(StorageCreateDirRequest {
            path: path("new/sibling")?,
            recursive: false,
        })
        .await?;
    assert_eq!(entry.entry_type(), EntryType::Directory);
    Ok(())
}

/// Assert directory cursor validation and ordered pagination on fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_pagination_contract(storage: &dyn Storage) -> TestResult {
    let oversized = NonZeroUsize::new(MAX_DIR_PAGE_LIMIT.get() + 1).ok_or("zero limit")?;
    for name in ["", "missing"] {
        let query = StorageDirQuery {
            path: path(name)?,
            after: None,
            limit: oversized,
        };
        let error = storage
            .list_entries(query)
            .await
            .err()
            .ok_or("expected oversized page")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::LimitExceeded));
        for after in ["", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
            assert!(ah_vfs_storage::EntryName::parse(after).is_err());
        }
    }
    let query = StorageDirQuery {
        path: path(&"x".repeat(ah_vfs_storage::MAX_PATH_BYTES))?,
        after: Some(ah_vfs_storage::EntryName::parse("child")?),
        limit: MAX_DIR_PAGE_LIMIT,
    };
    let error = storage
        .list_entries(query)
        .await
        .err()
        .ok_or("expected path overflow")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::PathTooLong));

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
    assert_eq!(page.entries[0].path(), &path("a")?);
    assert_eq!(
        page.next_after
            .as_ref()
            .map(ah_vfs_storage::EntryName::as_str),
        Some("a")
    );
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
            .map(|entry| entry.path().as_str())
            .collect::<Vec<_>>(),
        ["b", "文件"]
    );
    assert!(page.next_after.is_none());
    Ok(())
}

/// Assert same-path rename semantics for the modes advertised by fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_same_path_rename_contract(
    storage: &dyn Storage,
    support: RenameSupport,
) -> TestResult {
    put(storage, "file", b"file content").await?;
    mkdir(storage, "dir").await?;
    put(storage, "dir/child", b"child content").await?;

    for replace in [false, true] {
        let supported = if replace {
            support.supports(ah_vfs_storage::RenameMode::Replace)
        } else {
            support.supports(ah_vfs_storage::RenameMode::NoReplace)
        };
        if !supported {
            continue;
        }
        for name in ["file", "dir"] {
            let before = storage
                .stat_entry(
                    ah_vfs_storage::StatRequest::new(
                        (path(name)?).to_rooted_expr(),
                        ah_vfs_storage::SymlinkAction::Preserve,
                    ),
                    Default::default(),
                )
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
                assert_eq!(entry.path(), before.path());
                assert_eq!(entry.entry_type(), before.entry_type());
                assert_eq!(entry.modified(), before.modified());
            } else {
                assert_eq!(
                    result
                        .err()
                        .ok_or("expected existing destination")?
                        .failure(),
                    Failure::Rejected(Rejection::AlreadyExists)
                );
            }
            let after = storage
                .stat_entry(
                    ah_vfs_storage::StatRequest::new(
                        (path(name)?).to_rooted_expr(),
                        ah_vfs_storage::SymlinkAction::Preserve,
                    ),
                    Default::default(),
                )
                .await?;
            assert_eq!(after.path(), before.path());
            assert_eq!(after.entry_type(), before.entry_type());
            assert_eq!(after.modified(), before.modified());
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
        assert_eq!(error.failure(), Failure::Rejected(Rejection::NotFound));
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
        assert_eq!(error.failure(), Failure::Rejected(Rejection::RootProtected));
    }
    Ok(())
}
