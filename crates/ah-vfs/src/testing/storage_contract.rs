//! Storage behavior shared by independent driver test suites.

use crate::application::port::{Storage, StorageFileReader};
use crate::resolve::StorageOperations;
use crate::resolve::operand::{CreateDirectoryOperand, RenameOperand};
use crate::resolve::operand::{ListOperand, ListOperandTarget};
use crate::resolve::path::{PathExpr, RootedPath};
use ah_vfs_storage::RenameMode;
use ah_vfs_storage::RenameSupport;
use ah_vfs_storage::{EntryType, StoragePath};
use ah_vfs_storage::{Failure, Rejection};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn path(value: &str) -> TestResult<StoragePath> {
    Ok(StoragePath::parse(value)?)
}

async fn put(storage: &dyn Storage, name: &str, bytes: &[u8]) -> TestResult {
    let mut writer = storage
        .write_file(
            crate::resolve::operand::WriteOperand::new(
                path(name)?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
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
                crate::resolve::operand::ReadOperand::new(
                    path(name)?.to_rooted_expr(),
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
        .create_dir(
            crate::resolve::operand::CreateDirectoryOperand::new(
                path(name)?.to_rooted_expr(),
                crate::resolve::operand::CreateDirectoryMode::Recursive,
            ),
            Default::default(),
        )
        .await?;
    Ok(())
}

async fn exists(storage: &dyn Storage, name: &str) -> TestResult<bool> {
    match storage
        .stat_entry(
            crate::resolve::operand::StatOperand::new(
                path(name)?.to_rooted_expr(),
                crate::resolve::policy::SymlinkAction::Preserve,
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
                    crate::resolve::operand::ReadOperand::new(
                        path(name)?.to_rooted_expr(),
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
                    crate::resolve::operand::ReadOperand::new(
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
    use crate::resolve::operand::CreateDirectoryMode::{Recursive, Single};
    use ah_vfs_storage::MutationOutcome;

    let create = |operand: &str, mode| -> TestResult<_> {
        Ok(storage.create_dir(
            CreateDirectoryOperand::new(PathExpr::parse(operand)?, mode),
            Default::default(),
        ))
    };
    put(storage, "file", b"retained file").await?;
    mkdir(storage, "dir").await?;
    mkdir(storage, "dir/child").await?;

    for (name, mode, expected) in [
        (
            "file",
            Recursive,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        (
            "file/child",
            Recursive,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        (
            "file/child",
            Single,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        ("file", Single, Failure::Rejected(Rejection::AlreadyExists)),
        ("file/", Single, Failure::Rejected(Rejection::NotDirectory)),
        (
            "file/",
            Recursive,
            Failure::Rejected(Rejection::NotDirectory),
        ),
        ("dir", Single, Failure::Rejected(Rejection::AlreadyExists)),
        (
            "missing/child",
            Single,
            Failure::Rejected(Rejection::NotFound),
        ),
    ] {
        let error = create(name, mode)?
            .await
            .err()
            .ok_or("expected create_dir failure")?;
        assert_eq!(error.failure(), expected, "{name}: {mode:?}");
        assert_eq!(
            error.outcome(),
            MutationOutcome::Unchanged,
            "{name}: {mode:?}"
        );
    }
    assert!(!exists(storage, "missing").await?);

    for name in ["/", "dir/"] {
        let before = storage
            .stat_entry(
                crate::resolve::operand::StatOperand::new(
                    path(name.trim_matches('/'))?.to_rooted_expr(),
                    crate::resolve::policy::SymlinkAction::Preserve,
                ),
                Default::default(),
            )
            .await?;
        let entry = create(name, Recursive)?.await?;
        assert_eq!(entry.path(), before.path());
        assert_eq!(entry.entry_type(), EntryType::Directory);
        assert_eq!(entry.modified(), before.modified());
        let after = storage
            .stat_entry(
                crate::resolve::operand::StatOperand::new(
                    path(name.trim_matches('/'))?.to_rooted_expr(),
                    crate::resolve::policy::SymlinkAction::Preserve,
                ),
                Default::default(),
            )
            .await?;
        assert_eq!(after.entry_type(), before.entry_type());
        assert_eq!(after.modified(), before.modified());

        let error = create(name, Single)?
            .await
            .err()
            .ok_or("expected existing directory")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::AlreadyExists));
    }
    assert_eq!(get(storage, "file").await?, b"retained file");
    assert!(exists(storage, "dir/child").await?);

    let entry = create("new/parents/leaf", Recursive)?.await?;
    assert_eq!(entry.path(), &path("new/parents/leaf")?);
    assert_eq!(entry.entry_type(), EntryType::Directory);
    for name in ["new", "new/parents", "new/parents/leaf"] {
        assert_eq!(
            storage
                .stat_entry(
                    crate::resolve::operand::StatOperand::new(
                        path(name)?.to_rooted_expr(),
                        crate::resolve::policy::SymlinkAction::Preserve
                    ),
                    Default::default()
                )
                .await?
                .entry_type(),
            EntryType::Directory
        );
    }
    let entry = create("new/sibling", Single)?.await?;
    assert_eq!(entry.entry_type(), EntryType::Directory);
    let entry = create("before/../target/", Recursive)?.await?;
    assert_eq!(entry.path(), &path("target")?);
    assert!(exists(storage, "before").await?);
    assert!(!exists(storage, "before/target").await?);

    let error = create("visible/../file/child", Recursive)?
        .await
        .err()
        .ok_or("expected a non-directory parent")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::NotDirectory));
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(exists(storage, "visible").await?);
    Ok(())
}

/// Assert directory cursor validation and ordered pagination on fresh writable storage.
/// The fixture must be empty and isolated; the assertion writes files and dirs.
pub async fn assert_pagination_contract(storage: &dyn Storage) -> TestResult {
    use ah_vfs_storage::PageSize;
    assert!(PageSize::new(0).is_err());
    assert!(PageSize::new(PageSize::MAX + 1).is_err());
    for after in ["", ".", "..", "a/b", "a\\b", "a:b", "a\0b"] {
        assert!(ah_vfs_storage::EntryName::parse(after).is_err());
    }
    for name in ["a", "b", "文件"] {
        put(storage, name, b"").await?;
    }
    let first = ListOperand::new(
        ListOperandTarget::Path(StoragePath::root().to_rooted_expr()),
        PageSize::new(1)?,
    );
    let page = storage.list_entries(&first, Default::default()).await?;
    assert_eq!(page.entries().len(), 1);
    assert_eq!(page.entries()[0].path(), &path("a")?);
    let cursor = page.continuation().ok_or("expected continuation")?;
    assert_eq!(cursor.after().as_str(), "a");
    let next = ListOperand::new(
        ListOperandTarget::Continue(cursor),
        PageSize::new(PageSize::MAX)?,
    );
    let page = storage.list_entries(&next, Default::default()).await?;
    assert_eq!(
        page.entries()
            .iter()
            .map(|entry| entry.path().as_str())
            .collect::<Vec<_>>(),
        ["b", "文件"]
    );
    assert!(page.continuation().is_none());
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
    mkdir(storage, "dir/child").await?;

    for mode in [RenameMode::NoReplace, RenameMode::Replace] {
        if !support.supports(mode) {
            continue;
        }
        for name in ["file", "dir"] {
            let before = storage
                .stat_entry(
                    crate::resolve::operand::StatOperand::new(
                        path(name)?.to_rooted_expr(),
                        crate::resolve::policy::SymlinkAction::Preserve,
                    ),
                    Default::default(),
                )
                .await?;
            let result = storage
                .move_entry(
                    RenameOperand::new(
                        (path(name)?).to_rooted_expr(),
                        (path(name)?).to_rooted_expr(),
                        mode,
                    )?,
                    Default::default(),
                )
                .await;
            if mode == RenameMode::Replace {
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
                    crate::resolve::operand::StatOperand::new(
                        path(name)?.to_rooted_expr(),
                        crate::resolve::policy::SymlinkAction::Preserve,
                    ),
                    Default::default(),
                )
                .await?;
            assert_eq!(after.path(), before.path());
            assert_eq!(after.entry_type(), before.entry_type());
            assert_eq!(after.modified(), before.modified());
            assert_eq!(get(storage, "file").await?, b"file content");
            assert!(exists(storage, "dir/child").await?);
        }

        let error = storage
            .move_entry(
                RenameOperand::new(
                    (path("missing")?).to_rooted_expr(),
                    (path("missing")?).to_rooted_expr(),
                    mode,
                )?,
                Default::default(),
            )
            .await
            .err()
            .ok_or("expected missing source")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::NotFound));
        assert!(!exists(storage, "missing").await?);

        assert!(
            RenameOperand::new(
                StoragePath::root().to_rooted_expr(),
                path("target")?.to_rooted_expr(),
                mode
            )
            .is_err()
        );
        assert!(
            RenameOperand::new(
                path("source")?.to_rooted_expr(),
                StoragePath::root().to_rooted_expr(),
                mode
            )
            .is_err()
        );
    }
    Ok(())
}
