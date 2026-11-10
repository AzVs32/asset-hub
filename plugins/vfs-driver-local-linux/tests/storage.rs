#![cfg(target_os = "linux")]
use ah_vfs_storage::{BackendFailure, Failure, Rejection};
use ah_vfs_storage::{RemoveKind, RemoveRequest, RenameMode, RenameRequest};
mod common;

use ah_vfs::application::port::{Driver, Storage, StorageFileReader};
use ah_vfs_storage::WriteMode;
use ah_vfs_storage::{EntryType, StoragePath};

use sqlx::sqlite::SqlitePoolOptions;
use std::{error::Error, path::PathBuf, sync::Arc};
use vfs_driver_local_linux::LinuxLocalDriver;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Fixture {
    storage: Arc<dyn Storage>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture() -> TestResult<Fixture> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("root");
    std::fs::create_dir(&root)?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    let driver = LinuxLocalDriver::new()?;
    let registry = common::registry(&driver, pool.clone()).await?;
    let instance = registry.create(common::local_config(&root)?).await?;
    let storage = driver
        .open(registry.open_context(instance.id.clone()).await?)
        .await?;
    drop(driver);
    pool.close().await;
    Ok(Fixture {
        storage,
        root,
        _dir: dir,
    })
}

fn path(value: &str) -> Result<StoragePath, ah_vfs_storage::PathError> {
    StoragePath::parse(value)
}

async fn read_all(reader: &mut dyn StorageFileReader) -> TestResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        assert!(!chunk.is_empty());
        bytes.extend_from_slice(&chunk);
    }
    assert!(reader.next_chunk().await?.is_none());
    Ok(bytes)
}

#[tokio::test]
async fn metadata_and_sorted_pagination_use_instance_relative_paths() -> TestResult {
    let f = fixture().await?;
    std::fs::write(f.root.join("c.txt"), b"123")?;
    std::fs::write(f.root.join("a.txt"), b"a")?;
    std::fs::create_dir(f.root.join("b"))?;
    std::fs::write(f.root.join("文件"), b"utf8")?;
    let root = f
        .storage
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                path("")?.to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(root.entry_type(), EntryType::Directory);
    let file = f
        .storage
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                path("c.txt")?.to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(file.path().as_str(), "c.txt");
    assert_eq!(file.entry_type(), EntryType::file(Some(3)));
    assert!(file.modified().is_some());
    assert!(matches!(
        f.storage
            .stat_entry(ah_vfs_storage::StatRequest::new(path("missing")?.to_rooted_expr(), ah_vfs_storage::SymlinkAction::Preserve), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound)
    ));

    use ah_vfs_storage::{ListRequest, ListTarget, PageSize};
    let query = ListRequest::new(
        ListTarget::Path(path("")?.to_rooted_expr()),
        PageSize::new(2)?,
    );
    let first = f.storage.list_entries(&query, Default::default()).await?;
    assert_eq!(
        first
            .entries()
            .iter()
            .map(|n| n.path().as_str())
            .collect::<Vec<_>>(),
        ["a.txt", "b"]
    );
    assert_eq!(first.entries()[1].entry_type(), EntryType::Directory);
    let cursor = first.continuation().ok_or("expected continuation")?;
    assert_eq!(cursor.after().as_str(), "b");
    let second = f
        .storage
        .list_entries(
            &ListRequest::new(ListTarget::Continue(cursor), PageSize::new(2)?),
            Default::default(),
        )
        .await?;
    assert_eq!(
        second
            .entries()
            .iter()
            .map(|n| n.path().as_str())
            .collect::<Vec<_>>(),
        ["c.txt", "文件"]
    );
    assert!(second.continuation().is_none());
    assert!(
        f.storage
            .list_entries(
                &ah_vfs_storage::ListRequest::new(
                    ah_vfs_storage::ListTarget::Path(path("b")?.to_rooted_expr()),
                    Default::default()
                ),
                Default::default()
            )
            .await?
            .entries()
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn invalid_dir_queries_fail_explicitly() -> TestResult {
    let f = fixture().await?;
    assert!(ah_vfs_storage::PageSize::new(1001).is_err());
    assert!(ah_vfs_storage::EntryName::parse("../outside").is_err());
    std::fs::write(f.root.join("file"), b"x")?;
    assert!(matches!(
        f.storage
            .list_entries(&ah_vfs_storage::ListRequest::new(ah_vfs_storage::ListTarget::Path(path("file")?.to_rooted_expr()), Default::default()), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotDirectory)
    ));
    Ok(())
}

#[tokio::test]
async fn dir_creation_and_removal_preserve_root_and_nonempty_dirs() -> TestResult {
    let f = fixture().await?;
    assert!(matches!(
        f.storage
            .create_dir(ah_vfs_storage::CreateDirectoryRequest::new(path("a/b")?.to_rooted_expr(), ah_vfs_storage::CreateDirectoryMode::Single), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound)
    ));
    let entry = f
        .storage
        .create_dir(
            ah_vfs_storage::CreateDirectoryRequest::new(
                path("a/b")?.to_rooted_expr(),
                ah_vfs_storage::CreateDirectoryMode::Recursive,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(entry.entry_type(), EntryType::Directory);
    f.storage
        .create_dir(
            ah_vfs_storage::CreateDirectoryRequest::new(
                path("a/b")?.to_rooted_expr(),
                ah_vfs_storage::CreateDirectoryMode::Recursive,
            ),
            Default::default(),
        )
        .await?;
    assert!(matches!(
        f.storage
            .create_dir(ah_vfs_storage::CreateDirectoryRequest::new(path("a/b")?.to_rooted_expr(), ah_vfs_storage::CreateDirectoryMode::Single), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AlreadyExists)
    ));
    assert!(matches!(
        f.storage
            .remove_entry(RemoveRequest::new((path("a")?).to_rooted_expr(), RemoveKind::EmptyDirectory)?, Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::DirectoryNotEmpty)
    ));
    std::fs::write(f.root.join("a/b/file"), b"x")?;
    f.storage
        .remove_entry(
            RemoveRequest::new(
                (path("a/b/file")?).to_rooted_expr(),
                RemoveKind::NonDirectory,
            )?,
            Default::default(),
        )
        .await?;
    f.storage
        .remove_entry(
            RemoveRequest::new((path("a/b")?).to_rooted_expr(), RemoveKind::EmptyDirectory)?,
            Default::default(),
        )
        .await?;
    f.storage
        .remove_entry(
            RemoveRequest::new((path("a")?).to_rooted_expr(), RemoveKind::EmptyDirectory)?,
            Default::default(),
        )
        .await?;
    assert!(!f.root.join("a").exists());
    let root = f
        .storage
        .create_dir(
            ah_vfs_storage::CreateDirectoryRequest::new(
                path("")?.to_rooted_expr(),
                ah_vfs_storage::CreateDirectoryMode::Recursive,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(root.path(), &path("")?);
    assert_eq!(root.entry_type(), EntryType::Directory);
    assert!(matches!(
        f.storage
            .create_dir(ah_vfs_storage::CreateDirectoryRequest::new(path("")?.to_rooted_expr(), ah_vfs_storage::CreateDirectoryMode::Single), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AlreadyExists)
    ));
    assert!(
        RemoveRequest::new(
            StoragePath::root().to_rooted_expr(),
            RemoveKind::EmptyDirectory
        )
        .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn chunked_writes_publish_only_at_commit_and_reads_honor_ranges() -> TestResult {
    let f = fixture().await?;
    let bytes: Vec<_> = (0..(f.storage.chunk_size().get() * 2 + 17))
        .map(|n| (n % 251) as u8)
        .collect();
    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("data")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(&[]).await?;
    for chunk in bytes.chunks(7777) {
        writer.write_chunk(chunk).await?;
    }
    assert!(!f.root.join("data").exists());
    assert!(
        f.storage
            .list_entries(
                &ah_vfs_storage::ListRequest::new(
                    ah_vfs_storage::ListTarget::Path(path("")?.to_rooted_expr()),
                    Default::default()
                ),
                Default::default()
            )
            .await?
            .entries()
            .is_empty()
    );
    let metadata = writer.commit().await?;
    assert_eq!(metadata.path(), &path("data")?);
    assert_eq!(
        metadata.entry_type(),
        EntryType::file(Some(bytes.len() as u64))
    );
    assert_eq!(std::fs::read(f.root.join("data"))?, bytes);
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed)
    ));
    assert!(matches!(
        writer.abort().await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed)
    ));
    assert!(matches!(
        writer.write_chunk(b"late").await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed)
    ));

    let mut reader = f
        .storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                path("data")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    let mut actual = reader.next_chunk().await?.ok_or("missing chunk")?;
    assert!(actual.len() <= f.storage.chunk_size().get());
    for (offset, length, expected) in [
        (7, Some(11), bytes[7..18].to_vec()),
        (
            bytes.len() as u64 - 2,
            Some(100),
            bytes[bytes.len() - 2..].to_vec(),
        ),
        (bytes.len() as u64 + 100, None, Vec::new()),
        (0, Some(0), Vec::new()),
        (bytes.len() as u64, None, Vec::new()),
        (i64::MAX as u64 + 1, None, Vec::new()),
        (u64::MAX, None, Vec::new()),
        (u64::MAX, Some(0), Vec::new()),
    ] {
        let mut reader = f
            .storage
            .read_file(
                ah_vfs_storage::ReadRequest::new(
                    path("data")?.to_rooted_expr(),
                    ah_vfs_storage::ReadRange::new(offset, length)?,
                ),
                Default::default(),
            )
            .await?;
        assert_eq!(read_all(reader.as_mut()).await?, expected);
    }
    std::fs::write(f.root.join("replacement"), b"new version")?;
    std::fs::rename(f.root.join("replacement"), f.root.join("data"))?;
    actual.extend(read_all(reader.as_mut()).await?);
    assert_eq!(actual, bytes);
    Ok(())
}

#[tokio::test]
async fn unsupported_replacement_and_aborted_creation_leave_targets_unchanged() -> TestResult {
    let f = fixture().await?;
    std::fs::write(f.root.join("data"), b"original longer contents")?;
    assert!(matches!(
        f.storage
            .write_file(ah_vfs_storage::WriteRequest::new(path("data")?.to_rooted_expr(), ah_vfs_storage::WriteMode::CreateNew, ah_vfs_storage::Publication::Staged), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AlreadyExists)
    ));
    assert!(matches!(
        f.storage.write_file(ah_vfs_storage::WriteRequest::new(path("data")?.to_rooted_expr(), WriteMode::Replace, ah_vfs_storage::Publication::Staged), Default::default()).await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported)
    ));
    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("discard")?.to_rooted_expr(),
                WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"new").await?;
    assert_eq!(
        std::fs::read(f.root.join("data"))?,
        b"original longer contents"
    );
    writer.abort().await?;
    assert!(matches!(
        writer.write_chunk(b"x").await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed)
    ));
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed)
    ));
    assert!(matches!(
        writer.abort().await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed)
    ));
    assert_eq!(
        std::fs::read(f.root.join("data"))?,
        b"original longer contents"
    );
    assert!(!f.root.join("discard").exists());
    let mut empty = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("empty")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(empty.commit().await?.entry_type(), EntryType::file(Some(0)));
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 2);
    Ok(())
}

#[tokio::test]
async fn dropping_pending_writes_cleans_up_without_publishing() -> TestResult {
    let f = fixture().await?;
    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("pending")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"uncommitted").await?;
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 0);
    drop(writer);
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 0);
    Ok(())
}

#[tokio::test]
async fn concurrent_create_new_writers_cannot_overwrite_each_other() -> TestResult {
    let f = fixture().await?;
    let mut first = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("same")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    let mut second = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("same")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    first.write_chunk(b"first").await?;
    second.write_chunk(b"second").await?;
    let (first, second) = tokio::join!(first.commit(), second.commit());
    match (first, second) {
        (Ok(_), Err(error)) if error.failure() == Failure::Rejected(Rejection::Conflict) => {
            assert_eq!(std::fs::read(f.root.join("same"))?, b"first")
        }
        (Err(error), Ok(_)) if error.failure() == Failure::Rejected(Rejection::Conflict) => {
            assert_eq!(std::fs::read(f.root.join("same"))?, b"second")
        }
        results => return Err(format!("unexpected commit results: {results:?}").into()),
    }
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 1);
    Ok(())
}

#[tokio::test]
async fn failed_commit_is_terminal_and_preserves_a_concurrently_created_target() -> TestResult {
    let f = fixture().await?;
    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("data")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"pending").await?;
    std::fs::write(f.root.join("data"), b"external")?;
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::Conflict)
    ));
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed)
    ));
    assert_eq!(std::fs::read(f.root.join("data"))?, b"external");
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 1);

    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("directory")?.to_rooted_expr(),
                WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    std::fs::create_dir(f.root.join("directory"))?;
    assert!(
        matches!(writer.commit().await, Err(error) if error.failure() == Failure::Rejected(Rejection::Conflict))
    );
    assert!(f.root.join("directory").is_dir());
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 2);
    Ok(())
}

#[tokio::test]
async fn sessions_keep_resources_after_the_storage_handle_is_dropped() -> TestResult {
    let f = fixture().await?;
    std::fs::write(f.root.join("read"), b"retained")?;
    let mut reader = f
        .storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                path("read")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("write")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    drop(f.storage);
    assert_eq!(read_all(reader.as_mut()).await?, b"retained");
    writer.write_chunk(b"still open").await?;
    writer.commit().await?;
    assert_eq!(std::fs::read(f.root.join("write"))?, b"still open");
    Ok(())
}

#[tokio::test]
async fn wrong_entry_types_and_root_writes_are_rejected() -> TestResult {
    let f = fixture().await?;
    std::fs::create_dir(f.root.join("directory"))?;
    std::fs::write(f.root.join("file"), b"x")?;
    for name in ["", "directory"] {
        assert!(matches!(
            f.storage
                .read_file(ah_vfs_storage::ReadRequest::new(path(name)?.to_rooted_expr(), ah_vfs_storage::ReadRange::FULL), Default::default())
                .await,
            Err(error) if error.failure() == Failure::Rejected(Rejection::IsDirectory)
        ));
    }
    assert!(matches!(
        f.storage
            .write_file(ah_vfs_storage::WriteRequest::new(path("")?.to_rooted_expr(), ah_vfs_storage::WriteMode::CreateNew, ah_vfs_storage::Publication::Staged), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::RootProtected)
    ));
    assert!(matches!(
        f.storage
            .write_file(ah_vfs_storage::WriteRequest::new(path("directory")?.to_rooted_expr(), WriteMode::CreateNew, ah_vfs_storage::Publication::Staged), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AlreadyExists)
    ));
    assert!(matches!(
        f.storage
            .create_dir(ah_vfs_storage::CreateDirectoryRequest::new(path("file/sub")?.to_rooted_expr(), ah_vfs_storage::CreateDirectoryMode::Recursive), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotDirectory)
    ));
    assert!(matches!(
        f.storage
            .read_file(ah_vfs_storage::ReadRequest::new(path("missing")?.to_rooted_expr(), ah_vfs_storage::ReadRange::FULL), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound)
    ));
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[tokio::test]
async fn rename_handles_files_dirs_conflicts_and_replacement() -> TestResult {
    let f = fixture().await?;
    std::fs::write(f.root.join("a"), b"a")?;
    std::fs::write(f.root.join("b"), b"b")?;
    assert!(matches!(
        f.storage
            .move_entry(RenameRequest::new((path("a")?).to_rooted_expr(), (path("b")?).to_rooted_expr(), RenameMode::NoReplace)?, Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AlreadyExists)
    ));
    assert_eq!(std::fs::read(f.root.join("a"))?, b"a");
    assert_eq!(std::fs::read(f.root.join("b"))?, b"b");
    let error = f
        .storage
        .move_entry(
            RenameRequest::new(
                path("a")?.to_rooted_expr(),
                path("b")?.to_rooted_expr(),
                RenameMode::Replace,
            )?,
            Default::default(),
        )
        .await
        .err()
        .ok_or("unsafe replacement was accepted")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::Unsupported));
    assert_eq!(error.outcome(), ah_vfs_storage::MutationOutcome::Unchanged);
    assert_eq!(std::fs::read(f.root.join("a"))?, b"a");
    assert_eq!(std::fs::read(f.root.join("b"))?, b"b");
    f.storage
        .move_entry(
            RenameRequest::new(
                path("a")?.to_rooted_expr(),
                path("c")?.to_rooted_expr(),
                RenameMode::Replace,
            )?,
            Default::default(),
        )
        .await?;
    assert!(!f.root.join("a").exists());
    assert_eq!(std::fs::read(f.root.join("c"))?, b"a");
    std::fs::create_dir(f.root.join("old"))?;
    std::fs::write(f.root.join("old/child"), b"child")?;
    let entry = f
        .storage
        .move_entry(
            RenameRequest::new(
                (path("old")?).to_rooted_expr(),
                (path("new")?).to_rooted_expr(),
                RenameMode::NoReplace,
            )?,
            Default::default(),
        )
        .await?;
    assert_eq!(entry.entry_type(), EntryType::Directory);
    assert_eq!(std::fs::read(f.root.join("new/child"))?, b"child");
    assert!(
        f.storage
            .move_entry(
                RenameRequest::new(
                    (path("new")?).to_rooted_expr(),
                    (path("new/child/sub")?).to_rooted_expr(),
                    RenameMode::NoReplace
                )?,
                Default::default()
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn rename_uses_the_actual_slot_and_concurrent_arrivals_cannot_overwrite() -> TestResult {
    use ah_vfs::application::port::LookupContext;
    use ah_vfs_storage::{MutationOutcome, PathExpr};
    use std::os::unix::fs::symlink;

    let f = fixture().await?;
    std::fs::create_dir_all(f.root.join("work/directory/child"))?;
    symlink("/work", f.root.join("alias"))?;
    for name in ["first", "second"] {
        std::fs::write(f.root.join("work").join(name), name)?;
    }
    let rename = |source: &str, destination: &str, mode| -> TestResult<_> {
        Ok(f.storage.move_entry(
            RenameRequest::new(
                PathExpr::parse(source)?,
                PathExpr::parse(destination)?,
                mode,
            )?,
            LookupContext {
                working_directory: path("work")?,
                ..Default::default()
            },
        ))
    };
    let same = rename("directory/", "/alias/directory/", RenameMode::Replace)?.await?;
    assert_eq!(same.path(), &path("work/directory")?);
    assert!(f.root.join("work/directory/child").is_dir());
    let error = rename("directory", "/alias/directory", RenameMode::NoReplace)?
        .await
        .err()
        .ok_or("same slot allowed with NoReplace")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::AlreadyExists));
    let error = rename("first", "new/", RenameMode::NoReplace)?
        .await
        .err()
        .ok_or("file moved to a directory operand")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::NotDirectory));
    std::fs::create_dir(f.root.join("work/empty"))?;
    assert!(
        matches!(rename("empty", "directory", RenameMode::Replace)?.await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::DirectoryNotEmpty))
    );
    assert!(
        matches!(rename("directory", "empty", RenameMode::Replace)?.await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported))
    );
    assert!(f.root.join("work/directory/child").is_dir());
    assert!(f.root.join("work/empty").is_dir());

    let (first, second) = tokio::join!(
        rename("first", "winner", RenameMode::NoReplace)?,
        rename("second", "winner", RenameMode::NoReplace)?,
    );
    let loser = match (first, second) {
        (Ok(_), Err(error)) => {
            assert_eq!(std::fs::read(f.root.join("work/winner"))?, b"first");
            assert_eq!(std::fs::read(f.root.join("work/second"))?, b"second");
            error
        }
        (Err(error), Ok(_)) => {
            assert_eq!(std::fs::read(f.root.join("work/winner"))?, b"second");
            assert_eq!(std::fs::read(f.root.join("work/first"))?, b"first");
            error
        }
        _ => return Err("expected exactly one publication".into()),
    };
    assert_eq!(loser.failure(), Failure::Rejected(Rejection::AlreadyExists));
    assert_eq!(loser.outcome(), MutationOutcome::Unchanged);
    Ok(())
}

#[tokio::test]
async fn staging_names_are_hidden_and_cannot_be_accessed() -> TestResult {
    let f = fixture().await?;
    let _writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("pending")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 0);
    let name = ".asset-hub-write-reserved";
    assert!(
        f.storage
            .list_entries(
                &ah_vfs_storage::ListRequest::new(
                    ah_vfs_storage::ListTarget::Path(path("")?.to_rooted_expr()),
                    Default::default()
                ),
                Default::default()
            )
            .await?
            .entries()
            .is_empty()
    );
    assert!(matches!(
        f.storage
            .stat_entry(ah_vfs_storage::StatRequest::new(path(name)?.to_rooted_expr(), ah_vfs_storage::SymlinkAction::Preserve), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AccessDenied)
    ));
    assert!(matches!(
        f.storage
            .remove_entry(RemoveRequest::new((path(name)?).to_rooted_expr(), RemoveKind::NonDirectory)?, Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AccessDenied)
    ));
    assert!(matches!(
        f.storage
            .create_dir(ah_vfs_storage::CreateDirectoryRequest::new(path(".asset-hub-write-reserved")?.to_rooted_expr(), ah_vfs_storage::CreateDirectoryMode::Recursive), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AccessDenied)
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn lookup_policy_controls_links_without_escaping_the_instance() -> TestResult {
    use std::os::unix::fs::symlink;
    let f = fixture().await?;
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("file"), b"outside")?;
    symlink(outside.path(), f.root.join("linked"))?;
    symlink(outside.path().join("file"), f.root.join("leaf"))?;
    symlink(outside.path().join("missing"), f.root.join("dangling"))?;
    for name in ["linked", "leaf", "dangling"] {
        assert_eq!(
            f.storage
                .stat_entry(
                    ah_vfs_storage::StatRequest::new(
                        path(name)?.to_rooted_expr(),
                        ah_vfs_storage::SymlinkAction::Preserve
                    ),
                    Default::default()
                )
                .await?
                .entry_type(),
            EntryType::Symlink
        );
    }
    let target = f
        .storage
        .read_link(
            ah_vfs_storage::ReadLinkRequest::new(ah_vfs_storage::PathExpr::parse("/dangling")?),
            Default::default(),
        )
        .await?;
    use std::os::unix::ffi::OsStrExt;
    assert_eq!(
        target.as_bytes(),
        outside.path().join("missing").as_os_str().as_bytes()
    );
    assert!(matches!(
        f.storage.stat_entry(
            ah_vfs_storage::StatRequest::new(ah_vfs_storage::PathExpr::parse("/linked/file")?, ah_vfs_storage::SymlinkAction::FollowWithinInstance),
            Default::default(),
        ).await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound)
    ));
    let page = f
        .storage
        .list_entries(
            &ah_vfs_storage::ListRequest::new(
                ah_vfs_storage::ListTarget::Path(path("")?.to_rooted_expr()),
                Default::default(),
            ),
            Default::default(),
        )
        .await?;
    assert!(
        page.entries()
            .iter()
            .all(|n| n.entry_type() == EntryType::Symlink)
    );
    assert!(matches!(
        f.storage
            .read_file(ah_vfs_storage::ReadRequest::new(path("leaf")?.to_rooted_expr(), ah_vfs_storage::ReadRange::FULL), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::NotFound)
    ));
    assert!(matches!(
        f.storage
            .write_file(ah_vfs_storage::WriteRequest::new(path("leaf")?.to_rooted_expr(), WriteMode::Replace, ah_vfs_storage::Publication::Staged), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported)
    ));
    assert!(matches!(
        f.storage
            .write_file(ah_vfs_storage::WriteRequest::new(path("dangling")?.to_rooted_expr(), ah_vfs_storage::WriteMode::CreateNew, ah_vfs_storage::Publication::Staged), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::AlreadyExists)
    ));
    assert!(matches!(
        f.storage
            .stat_entry(ah_vfs_storage::StatRequest::new(path("linked/file")?.to_rooted_expr(), ah_vfs_storage::SymlinkAction::Preserve), ah_vfs::application::port::LookupContext {
                intermediate_symlinks: ah_vfs_storage::IntermediateSymlinks::Reject,
                ..Default::default()
            })
            .await,
        Err(error) if error.failure() == Failure::Rejected(Rejection::SymlinkNotAllowed)
    ));
    assert!(
        f.storage
            .list_entries(
                &ah_vfs_storage::ListRequest::new(
                    ah_vfs_storage::ListTarget::Path(path("linked")?.to_rooted_expr()),
                    Default::default()
                ),
                Default::default()
            )
            .await
            .is_err()
    );
    assert!(
        f.storage
            .read_file(
                ah_vfs_storage::ReadRequest::new(
                    path("linked/file")?.to_rooted_expr(),
                    ah_vfs_storage::ReadRange::FULL
                ),
                Default::default()
            )
            .await
            .is_err()
    );
    assert!(
        f.storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    path("linked/new")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged
                ),
                Default::default()
            )
            .await
            .is_err()
    );
    assert!(
        f.storage
            .create_dir(
                ah_vfs_storage::CreateDirectoryRequest::new(
                    path("linked/new")?.to_rooted_expr(),
                    ah_vfs_storage::CreateDirectoryMode::Recursive
                ),
                Default::default()
            )
            .await
            .is_err()
    );
    assert!(
        f.storage
            .remove_entry(
                RemoveRequest::new(
                    (path("linked/file")?).to_rooted_expr(),
                    RemoveKind::NonDirectory
                )?,
                Default::default()
            )
            .await
            .is_err()
    );
    assert!(
        f.storage
            .move_entry(
                RenameRequest::new(
                    (path("linked/file")?).to_rooted_expr(),
                    (path("moved")?).to_rooted_expr(),
                    RenameMode::Replace
                )?,
                Default::default()
            )
            .await
            .is_err()
    );
    f.storage
        .move_entry(
            RenameRequest::new(
                (path("leaf")?).to_rooted_expr(),
                (path("moved-link")?).to_rooted_expr(),
                RenameMode::Replace,
            )?,
            Default::default(),
        )
        .await?;
    assert!(
        std::fs::symlink_metadata(f.root.join("moved-link"))?
            .file_type()
            .is_symlink()
    );
    f.storage
        .remove_entry(
            RemoveRequest::new(
                (path("moved-link")?).to_rooted_expr(),
                RemoveKind::NonDirectory,
            )?,
            Default::default(),
        )
        .await?;
    f.storage
        .remove_entry(
            RemoveRequest::new((path("linked")?).to_rooted_expr(), RemoveKind::NonDirectory)?,
            Default::default(),
        )
        .await?;
    f.storage
        .remove_entry(
            RemoveRequest::new(
                (path("dangling")?).to_rooted_expr(),
                RemoveKind::NonDirectory,
            )?,
            Default::default(),
        )
        .await?;
    assert_eq!(std::fs::read(outside.path().join("file"))?, b"outside");
    assert!(!outside.path().join("new").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn nested_writes_without_atomic_parent_binding_are_rejected_before_staging() -> TestResult {
    let f = fixture().await?;
    std::fs::create_dir(f.root.join("parent"))?;
    let request = ah_vfs_storage::WriteRequest::new(
        path("parent/file")?.to_rooted_expr(),
        WriteMode::CreateNew,
        ah_vfs_storage::Publication::Atomic,
    );
    assert!(
        matches!(f.storage.write_file(request, Default::default()).await,
            Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported)
        )
    );
    assert_eq!(std::fs::read_dir(f.root.join("parent"))?.count(), 0);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_dir_entries_fail_without_lossy_conversion() -> TestResult {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let f = fixture().await?;
    std::fs::write(f.root.join(OsString::from_vec(b"file-\xff".to_vec())), b"x")?;
    assert!(matches!(
        f.storage
            .list_entries(&ah_vfs_storage::ListRequest::new(ah_vfs_storage::ListTarget::Path(path("")?.to_rooted_expr()), Default::default()), Default::default())
            .await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::UnrepresentableName)
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn new_files_are_private() -> TestResult {
    use std::os::unix::fs::MetadataExt;
    let f = fixture().await?;
    let mut writer = f
        .storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("new")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.commit().await?;
    assert_eq!(std::fs::metadata(f.root.join("new"))?.mode() & 0o777, 0o600);
    Ok(())
}

#[tokio::test]
async fn recursive_creation_failure_preserves_cause_operation_and_parent_effects() -> TestResult {
    use ah_vfs_storage::{MutationOutcome, Operation};
    let f = fixture().await?;
    let request = ah_vfs_storage::CreateDirectoryRequest::new(
        (path(&format!("created/{}", "x".repeat(256)))?).to_rooted_expr(),
        ah_vfs_storage::CreateDirectoryMode::Recursive,
    );
    let error = f
        .storage
        .create_dir(request, Default::default())
        .await
        .err()
        .ok_or("native name limit accepted")?;
    assert_eq!(error.operation(), Operation::CreateDirectory);
    assert_eq!(error.failure(), Failure::Rejected(Rejection::NameTooLong));
    assert_eq!(error.outcome(), MutationOutcome::Unknown);
    assert!(f.root.join("created").is_dir());
    assert_eq!(std::fs::read_dir(f.root.join("created"))?.count(), 0);

    Ok(())
}

#[tokio::test]
async fn recursive_creation_follows_existing_links_without_creating_dangling_targets() -> TestResult
{
    use ah_vfs_storage::{CreateDirectoryMode, CreateDirectoryRequest, MutationOutcome, PathExpr};
    use std::os::unix::fs::symlink;

    let f = fixture().await?;
    std::fs::create_dir(f.root.join("existing"))?;
    symlink("/existing", f.root.join("alias"))?;
    symlink("/absent/deeper", f.root.join("dangling"))?;
    let create = |operand: &str, mode| -> TestResult<_> {
        Ok(f.storage.create_dir(
            CreateDirectoryRequest::new(PathExpr::parse(operand)?, mode),
            Default::default(),
        ))
    };

    let entry = create("alias/child/", CreateDirectoryMode::Recursive)?.await?;
    assert_eq!(entry.path(), &path("existing/child")?);
    assert!(f.root.join("existing/child").is_dir());
    for (operand, outcome) in [
        ("dangling", MutationOutcome::Unchanged),
        ("dangling/child", MutationOutcome::Unchanged),
        ("visible/../dangling/child", MutationOutcome::Unknown),
    ] {
        let error = create(operand, CreateDirectoryMode::Recursive)?
            .await
            .err()
            .ok_or("dangling link accepted")?;
        assert_eq!(error.failure(), Failure::Rejected(Rejection::NotFound));
        assert_eq!(error.outcome(), outcome);
    }
    assert!(f.root.join("visible").is_dir());
    assert!(!f.root.join("absent").exists());

    let error = create("dangling", CreateDirectoryMode::Single)?
        .await
        .err()
        .ok_or("existing link overwritten")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::AlreadyExists));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    Ok(())
}

#[tokio::test]
async fn directory_cursor_survives_alias_changes_but_rejects_replacement_and_other_instances()
-> TestResult {
    use ah_vfs_storage::{ListRequest, ListTarget, PageSize, PathExpr};
    use std::os::unix::fs::symlink;
    let first = fixture().await?;
    let other = fixture().await?;
    for root in [&first.root, &other.root] {
        std::fs::create_dir(root.join("a"))?;
        std::fs::write(root.join("a/one"), b"one")?;
        std::fs::write(root.join("a/two"), b"two")?;
    }
    std::fs::create_dir(first.root.join("b"))?;
    symlink("/a", first.root.join("alias"))?;
    let initial = ListRequest::new(
        ListTarget::Path(PathExpr::parse("/alias")?),
        PageSize::new(1)?,
    );
    let page = first
        .storage
        .list_entries(&initial, Default::default())
        .await?;
    assert_eq!(page.selection().address(), &path("a")?);
    assert_eq!(page.entries()[0].path(), &path("a/one")?);
    let resume = ListRequest::new(
        ListTarget::Continue(page.continuation().ok_or("continuation")?),
        PageSize::DEFAULT,
    );
    std::fs::remove_file(first.root.join("alias"))?;
    symlink("/b", first.root.join("alias"))?;
    let page = first
        .storage
        .list_entries(&resume, Default::default())
        .await?;
    assert_eq!(page.entries()[0].path(), &path("a/two")?);
    assert!(page.continuation().is_none());
    assert!(
        matches!(other.storage.list_entries(&resume, Default::default()).await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Stale))
    );
    std::fs::remove_dir_all(first.root.join("a"))?;
    std::fs::create_dir(first.root.join("a"))?;
    assert!(
        matches!(first.storage.list_entries(&resume, Default::default()).await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Stale))
    );
    Ok(())
}
