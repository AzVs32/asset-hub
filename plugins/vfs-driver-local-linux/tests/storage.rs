#![cfg(target_os = "linux")]
mod common;

use ah_vfs::{
    application::port::{Driver, Storage, StorageFileReader},
    domain::storage::{
        EntryKind, StorageCreateDirRequest, StorageDirQuery, StorageErrorKind,
        StorageMoveEntryRequest, StoragePath, StorageReadFileRequest, StorageRemoveEntryRequest,
        StorageStatEntryRequest, StorageWriteFileMode, StorageWriteFileRequest,
    },
};

use sqlx::sqlite::SqlitePoolOptions;
use std::{error::Error, num::NonZeroUsize, path::PathBuf, sync::Arc};
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

fn path(value: &str) -> Result<StoragePath, ah_vfs::domain::storage::StoragePathError> {
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
        .stat_entry(StorageStatEntryRequest { path: path("")? })
        .await?;
    assert_eq!(root.kind, EntryKind::Dir);
    assert_eq!(root.size, None);
    let file = f
        .storage
        .stat_entry(StorageStatEntryRequest {
            path: path("c.txt")?,
        })
        .await?;
    assert_eq!(file.path.as_str(), "c.txt");
    assert_eq!(file.kind, EntryKind::File);
    assert_eq!(file.size, Some(3));
    assert!(file.modified.is_some());
    assert!(matches!(
        f.storage
            .stat_entry(StorageStatEntryRequest {
                path: path("missing")?
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::NotFound
    ));

    let mut query = StorageDirQuery::new(path("")?);
    query.limit = NonZeroUsize::new(2).ok_or("zero limit")?;
    let first = f.storage.list_entries(query).await?;
    assert_eq!(
        first
            .entries
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["a.txt", "b"]
    );
    assert_eq!(first.entries[1].kind, EntryKind::Dir);
    assert_eq!(first.next_after.as_deref(), Some("b"));
    let second = f
        .storage
        .list_entries(StorageDirQuery {
            path: path("")?,
            after: first.next_after,
            limit: NonZeroUsize::new(2).ok_or("zero limit")?,
        })
        .await?;
    assert_eq!(
        second
            .entries
            .iter()
            .map(|n| n.path.as_str())
            .collect::<Vec<_>>(),
        ["c.txt", "文件"]
    );
    assert!(second.next_after.is_none());
    assert!(
        f.storage
            .list_entries(StorageDirQuery::new(path("b")?))
            .await?
            .entries
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn invalid_dir_queries_fail_explicitly() -> TestResult {
    let f = fixture().await?;
    let mut query = StorageDirQuery::new(path("")?);
    query.limit = NonZeroUsize::new(1001).ok_or("zero limit")?;
    assert!(matches!(
        f.storage.list_entries(query).await,
        Err(error) if error.kind() == StorageErrorKind::InvalidRequest
    ));
    let mut query = StorageDirQuery::new(path("")?);
    query.after = Some("../outside".into());
    assert!(matches!(
        f.storage.list_entries(query).await,
        Err(error) if error.kind() == StorageErrorKind::InvalidPath
    ));
    std::fs::write(f.root.join("file"), b"x")?;
    assert!(matches!(
        f.storage
            .list_entries(StorageDirQuery::new(path("file")?))
            .await,
        Err(error) if error.kind() == StorageErrorKind::NotDir
    ));
    Ok(())
}

#[tokio::test]
async fn dir_creation_and_removal_preserve_root_and_nonempty_dirs() -> TestResult {
    let f = fixture().await?;
    assert!(matches!(
        f.storage
            .create_dir(StorageCreateDirRequest {
                path: path("a/b")?,
                recursive: false,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::NotFound
    ));
    let entry = f
        .storage
        .create_dir(StorageCreateDirRequest {
            path: path("a/b")?,
            recursive: true,
        })
        .await?;
    assert_eq!(entry.kind, EntryKind::Dir);
    f.storage
        .create_dir(StorageCreateDirRequest {
            path: path("a/b")?,
            recursive: true,
        })
        .await?;
    assert!(matches!(
        f.storage
            .create_dir(StorageCreateDirRequest {
                path: path("a/b")?,
                recursive: false,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::AlreadyExists
    ));
    assert!(matches!(
        f.storage
            .remove_entry(StorageRemoveEntryRequest { path: path("a")? })
            .await,
        Err(error) if error.kind() == StorageErrorKind::DirNotEmpty
    ));
    std::fs::write(f.root.join("a/b/file"), b"x")?;
    f.storage
        .remove_entry(StorageRemoveEntryRequest {
            path: path("a/b/file")?,
        })
        .await?;
    f.storage
        .remove_entry(StorageRemoveEntryRequest { path: path("a/b")? })
        .await?;
    f.storage
        .remove_entry(StorageRemoveEntryRequest { path: path("a")? })
        .await?;
    assert!(!f.root.join("a").exists());
    let root = f
        .storage
        .create_dir(StorageCreateDirRequest {
            path: path("")?,
            recursive: true,
        })
        .await?;
    assert_eq!(root.path, path("")?);
    assert_eq!(root.kind, EntryKind::Dir);
    assert!(matches!(
        f.storage
            .create_dir(StorageCreateDirRequest {
                path: path("")?,
                recursive: false,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::AlreadyExists
    ));
    assert!(matches!(
        f.storage
            .remove_entry(StorageRemoveEntryRequest { path: path("")? })
            .await,
        Err(error) if error.kind() == StorageErrorKind::RootProtected
    ));
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
        .write_file(StorageWriteFileRequest::new(path("data")?))
        .await?;
    writer.write_chunk(&[]).await?;
    for chunk in bytes.chunks(7777) {
        writer.write_chunk(chunk).await?;
    }
    assert!(!f.root.join("data").exists());
    assert!(
        f.storage
            .list_entries(StorageDirQuery::new(path("")?))
            .await?
            .entries
            .is_empty()
    );
    let metadata = writer.commit().await?;
    assert_eq!(metadata.path, path("data")?);
    assert_eq!(metadata.size, Some(bytes.len() as u64));
    assert_eq!(std::fs::read(f.root.join("data"))?, bytes);
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.kind() == StorageErrorKind::SessionClosed
    ));
    assert!(matches!(
        writer.abort().await,
        Err(error) if error.kind() == StorageErrorKind::SessionClosed
    ));
    assert!(matches!(
        writer.write_chunk(b"late").await,
        Err(error) if error.kind() == StorageErrorKind::SessionClosed
    ));

    let mut reader = f
        .storage
        .read_file(StorageReadFileRequest::new(path("data")?))
        .await?;
    let first = reader.next_chunk().await?.ok_or("missing chunk")?;
    assert!(first.len() <= f.storage.chunk_size().get());
    let mut actual = first;
    actual.extend(read_all(reader.as_mut()).await?);
    assert_eq!(actual, bytes);
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
        (u64::MAX, Some(1), Vec::new()),
    ] {
        let mut reader = f
            .storage
            .read_file(StorageReadFileRequest {
                path: path("data")?,
                offset,
                length,
            })
            .await?;
        assert_eq!(read_all(reader.as_mut()).await?, expected);
    }
    Ok(())
}

#[tokio::test]
async fn replacing_and_aborting_keep_existing_data_until_publication() -> TestResult {
    let f = fixture().await?;
    std::fs::write(f.root.join("data"), b"original longer contents")?;
    assert!(matches!(
        f.storage
            .write_file(StorageWriteFileRequest::new(path("data")?))
            .await,
        Err(error) if error.kind() == StorageErrorKind::AlreadyExists
    ));
    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest {
            require_atomic_publish: false,
            path: path("data")?,
            mode: StorageWriteFileMode::Replace,
        })
        .await?;
    writer.write_chunk(b"new").await?;
    assert_eq!(
        std::fs::read(f.root.join("data"))?,
        b"original longer contents"
    );
    writer.abort().await?;
    assert!(matches!(
        writer.write_chunk(b"x").await,
        Err(error) if error.kind() == StorageErrorKind::SessionClosed
    ));
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.kind() == StorageErrorKind::SessionClosed
    ));
    assert!(matches!(
        writer.abort().await,
        Err(error) if error.kind() == StorageErrorKind::SessionClosed
    ));
    assert_eq!(
        std::fs::read(f.root.join("data"))?,
        b"original longer contents"
    );
    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest {
            require_atomic_publish: false,
            path: path("data")?,
            mode: StorageWriteFileMode::Replace,
        })
        .await?;
    writer.write_chunk(b"new").await?;
    writer.commit().await?;
    assert_eq!(std::fs::read(f.root.join("data"))?, b"new");
    let mut empty = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("empty")?))
        .await?;
    assert_eq!(empty.commit().await?.size, Some(0));
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 2);
    Ok(())
}

#[tokio::test]
async fn dropping_pending_writes_cleans_up_without_publishing() -> TestResult {
    let f = fixture().await?;
    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("pending")?))
        .await?;
    writer.write_chunk(b"uncommitted").await?;
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 1);
    drop(writer);
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 0);
    Ok(())
}

#[tokio::test]
async fn concurrent_create_new_writers_cannot_overwrite_each_other() -> TestResult {
    let f = fixture().await?;
    let mut first = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("same")?))
        .await?;
    let mut second = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("same")?))
        .await?;
    first.write_chunk(b"first").await?;
    second.write_chunk(b"second").await?;
    let (first, second) = tokio::join!(first.commit(), second.commit());
    match (first, second) {
        (Ok(_), Err(error)) if error.kind() == StorageErrorKind::AlreadyExists => {
            assert_eq!(std::fs::read(f.root.join("same"))?, b"first")
        }
        (Err(error), Ok(_)) if error.kind() == StorageErrorKind::AlreadyExists => {
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
        .write_file(StorageWriteFileRequest::new(path("data")?))
        .await?;
    writer.write_chunk(b"pending").await?;
    std::fs::write(f.root.join("data"), b"external")?;
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.kind() == StorageErrorKind::AlreadyExists
    ));
    assert!(matches!(
        writer.commit().await,
        Err(error) if error.kind() == StorageErrorKind::SessionClosed
    ));
    assert_eq!(std::fs::read(f.root.join("data"))?, b"external");
    assert_eq!(std::fs::read_dir(&f.root)?.count(), 1);

    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest {
            require_atomic_publish: false,
            path: path("directory")?,
            mode: StorageWriteFileMode::Replace,
        })
        .await?;
    std::fs::create_dir(f.root.join("directory"))?;
    assert!(matches!(writer.commit().await, Err(error) if error.kind() == StorageErrorKind::IsDir));
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
        .read_file(StorageReadFileRequest::new(path("read")?))
        .await?;
    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("write")?))
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
                .read_file(StorageReadFileRequest::new(path(name)?))
                .await,
            Err(error) if error.kind() == StorageErrorKind::IsDir
        ));
    }
    assert!(matches!(
        f.storage
            .write_file(StorageWriteFileRequest::new(path("")?))
            .await,
        Err(error) if error.kind() == StorageErrorKind::RootProtected
    ));
    assert!(matches!(
        f.storage
            .write_file(StorageWriteFileRequest { require_atomic_publish: false,
                path: path("directory")?,
                mode: StorageWriteFileMode::Replace,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::IsDir
    ));
    assert!(matches!(
        f.storage
            .create_dir(StorageCreateDirRequest {
                path: path("file/sub")?,
                recursive: true,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::NotDir
    ));
    assert!(matches!(
        f.storage
            .read_file(StorageReadFileRequest::new(path("missing")?))
            .await,
        Err(error) if error.kind() == StorageErrorKind::NotFound
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
            .move_entry(StorageMoveEntryRequest {
                source: path("a")?,
                destination: path("b")?,
                replace: false,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::AlreadyExists
    ));
    assert_eq!(std::fs::read(f.root.join("a"))?, b"a");
    assert_eq!(std::fs::read(f.root.join("b"))?, b"b");
    let entry = f
        .storage
        .move_entry(StorageMoveEntryRequest {
            source: path("a")?,
            destination: path("b")?,
            replace: true,
        })
        .await?;
    assert_eq!(entry.path, path("b")?);
    assert!(!f.root.join("a").exists());
    assert_eq!(std::fs::read(f.root.join("b"))?, b"a");
    f.storage
        .move_entry(StorageMoveEntryRequest {
            source: path("b")?,
            destination: path("c")?,
            replace: false,
        })
        .await?;
    std::fs::create_dir(f.root.join("old"))?;
    std::fs::write(f.root.join("old/child"), b"child")?;
    let entry = f
        .storage
        .move_entry(StorageMoveEntryRequest {
            source: path("old")?,
            destination: path("new")?,
            replace: false,
        })
        .await?;
    assert_eq!(entry.kind, EntryKind::Dir);
    assert_eq!(std::fs::read(f.root.join("new/child"))?, b"child");
    assert!(
        f.storage
            .move_entry(StorageMoveEntryRequest {
                source: path("new")?,
                destination: path("new/child/sub")?,
                replace: false,
            })
            .await
            .is_err()
    );
    assert!(matches!(
        f.storage
            .move_entry(StorageMoveEntryRequest {
                source: path("")?,
                destination: path("root")?,
                replace: false,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::RootProtected
    ));
    assert!(matches!(
        f.storage
            .move_entry(StorageMoveEntryRequest {
                source: path("c")?,
                destination: path("")?,
                replace: true,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::RootProtected
    ));
    Ok(())
}

#[tokio::test]
async fn staging_names_are_hidden_and_cannot_be_accessed() -> TestResult {
    let f = fixture().await?;
    let _writer = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("pending")?))
        .await?;
    let staging = std::fs::read_dir(&f.root)?
        .next()
        .ok_or("missing staging file")??;
    let name = staging
        .file_name()
        .into_string()
        .map_err(|_| "invalid staging name")?;
    assert!(
        f.storage
            .list_entries(StorageDirQuery::new(path("")?))
            .await?
            .entries
            .is_empty()
    );
    assert!(matches!(
        f.storage
            .stat_entry(StorageStatEntryRequest { path: path(&name)? })
            .await,
        Err(error) if error.kind() == StorageErrorKind::PermissionDenied
    ));
    assert!(matches!(
        f.storage
            .remove_entry(StorageRemoveEntryRequest { path: path(&name)? })
            .await,
        Err(error) if error.kind() == StorageErrorKind::PermissionDenied
    ));
    assert!(matches!(
        f.storage
            .create_dir(StorageCreateDirRequest {
                path: path(".asset-hub-write-reserved")?,
                recursive: true,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::PermissionDenied
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_are_visible_as_links_but_never_followed() -> TestResult {
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
                .stat_entry(StorageStatEntryRequest { path: path(name)? })
                .await?
                .kind,
            EntryKind::Symlink
        );
    }
    let page = f
        .storage
        .list_entries(StorageDirQuery::new(path("")?))
        .await?;
    assert!(page.entries.iter().all(|n| n.kind == EntryKind::Symlink));
    assert!(matches!(
        f.storage
            .read_file(StorageReadFileRequest::new(path("leaf")?))
            .await,
        Err(error) if error.kind() == StorageErrorKind::UnsupportedOperation
    ));
    assert!(matches!(
        f.storage
            .write_file(StorageWriteFileRequest { require_atomic_publish: false,
                path: path("leaf")?,
                mode: StorageWriteFileMode::Replace,
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::UnsupportedOperation
    ));
    assert!(matches!(
        f.storage
            .write_file(StorageWriteFileRequest::new(path("dangling")?))
            .await,
        Err(error) if error.kind() == StorageErrorKind::AlreadyExists
    ));
    assert!(matches!(
        f.storage
            .stat_entry(StorageStatEntryRequest {
                path: path("linked/file")?
            })
            .await,
        Err(error) if error.kind() == StorageErrorKind::UnsupportedOperation
    ));
    assert!(
        f.storage
            .list_entries(StorageDirQuery::new(path("linked")?))
            .await
            .is_err()
    );
    assert!(
        f.storage
            .read_file(StorageReadFileRequest::new(path("linked/file")?))
            .await
            .is_err()
    );
    assert!(
        f.storage
            .write_file(StorageWriteFileRequest::new(path("linked/new")?))
            .await
            .is_err()
    );
    assert!(
        f.storage
            .create_dir(StorageCreateDirRequest {
                path: path("linked/new")?,
                recursive: true,
            })
            .await
            .is_err()
    );
    assert!(
        f.storage
            .remove_entry(StorageRemoveEntryRequest {
                path: path("linked/file")?
            })
            .await
            .is_err()
    );
    assert!(
        f.storage
            .move_entry(StorageMoveEntryRequest {
                source: path("linked/file")?,
                destination: path("moved")?,
                replace: true,
            })
            .await
            .is_err()
    );
    f.storage
        .move_entry(StorageMoveEntryRequest {
            source: path("leaf")?,
            destination: path("moved-link")?,
            replace: true,
        })
        .await?;
    assert!(
        std::fs::symlink_metadata(f.root.join("moved-link"))?
            .file_type()
            .is_symlink()
    );
    f.storage
        .remove_entry(StorageRemoveEntryRequest {
            path: path("moved-link")?,
        })
        .await?;
    f.storage
        .remove_entry(StorageRemoveEntryRequest {
            path: path("linked")?,
        })
        .await?;
    f.storage
        .remove_entry(StorageRemoveEntryRequest {
            path: path("dangling")?,
        })
        .await?;
    assert_eq!(std::fs::read(outside.path().join("file"))?, b"outside");
    assert!(!outside.path().join("new").exists());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn an_open_writer_pins_its_parent_even_when_the_dir_is_replaced_by_a_link() -> TestResult {
    use std::os::unix::fs::symlink;
    let f = fixture().await?;
    let outside = tempfile::tempdir()?;
    std::fs::create_dir(f.root.join("parent"))?;
    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("parent/file")?))
        .await?;
    writer.write_chunk(b"inside").await?;
    std::fs::rename(f.root.join("parent"), f.root.join("original-parent"))?;
    symlink(outside.path(), f.root.join("parent"))?;
    writer.commit().await?;
    assert_eq!(
        std::fs::read(f.root.join("original-parent/file"))?,
        b"inside"
    );
    assert!(!outside.path().join("file").exists());
    assert!(
        f.storage
            .read_file(StorageReadFileRequest::new(path("parent/file")?))
            .await
            .is_err()
    );
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
            .list_entries(StorageDirQuery::new(path("")?))
            .await,
        Err(error) if error.kind() == StorageErrorKind::InvalidNameEncoding
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn new_files_are_private_and_replacement_preserves_destination_permissions() -> TestResult {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let f = fixture().await?;
    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest::new(path("new")?))
        .await?;
    writer.commit().await?;
    assert_eq!(std::fs::metadata(f.root.join("new"))?.mode() & 0o777, 0o600);
    std::fs::set_permissions(f.root.join("new"), std::fs::Permissions::from_mode(0o640))?;
    let mut writer = f
        .storage
        .write_file(StorageWriteFileRequest {
            require_atomic_publish: false,
            path: path("new")?,
            mode: StorageWriteFileMode::Replace,
        })
        .await?;
    writer.write_chunk(b"replacement").await?;
    writer.commit().await?;
    assert_eq!(std::fs::metadata(f.root.join("new"))?.mode() & 0o777, 0o640);
    Ok(())
}
