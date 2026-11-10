use ah_vfs_storage::{BackendFailure, Failure, Rejection};
mod common;

use ah_vfs::application::port::{Driver, Storage, validate_storage_extensions};
use ah_vfs::domain::storage::{StorageMoveEntryRequest, StorageRemoveEntryRequest};
use ah_vfs_storage::EntryType;
use ah_vfs_storage::{AttributeSupport, WriteMode};
use common::*;
use std::sync::Arc;
use vfs_driver_memory::{Capacity, CapacityUnit, MemoryDriver, MemoryStorage};

fn storage(bytes: u64) -> TestResult<MemoryStorage> {
    Ok(MemoryStorage::new(Capacity::new(bytes, CapacityUnit::B)?))
}

#[tokio::test]
async fn declarations_cover_all_ports_and_optional_attributes_are_absent() -> TestResult {
    let storage = storage(32)?;
    let capabilities = MemoryDriver::new()?.info().capabilities();
    validate_storage_extensions(capabilities, &storage)?;
    assert_eq!(capabilities.permissions(), AttributeSupport::None);
    assert_eq!(capabilities.xattrs(), AttributeSupport::None);
    assert!(storage.permissions().is_none());
    assert!(storage.xattrs().is_none());
    assert_eq!(storage.chunk_size().get(), 64 * 1024);
    let root = storage
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                path("")?.to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(root.entry_type(), EntryType::Directory);
    Ok(())
}

#[tokio::test]
async fn publication_is_staged_and_commit_is_terminal() -> TestResult {
    let storage = storage(64)?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("file")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Atomic,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"one").await?;
    writer.write_chunk(b"two").await?;
    assert!(!exists(&storage, "file").await?);
    assert_eq!(storage.used_bytes(), 6);
    let metadata = writer.commit().await?;
    assert_eq!(metadata.path(), &path("file")?);
    assert_eq!(metadata.entry_type(), EntryType::file(Some(6)));
    assert_eq!(get(&storage, "file").await?, b"onetwo");
    assert_eq!(
        writer
            .commit()
            .await
            .err()
            .ok_or("expected closed")?
            .failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    assert_eq!(
        writer
            .abort()
            .await
            .err()
            .ok_or("expected closed")?
            .failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    assert_eq!(
        writer
            .write_chunk(b"x")
            .await
            .err()
            .ok_or("expected closed")?
            .failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    Ok(())
}

#[tokio::test]
async fn abort_drop_and_unpolled_operations_never_publish_or_leak_quota() -> TestResult {
    let storage = storage(8)?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("abort")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"1234").await?;
    writer.abort().await?;
    assert_eq!(storage.used_bytes(), 0);
    assert_eq!(
        writer
            .abort()
            .await
            .err()
            .ok_or("expected closed")?
            .failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    {
        let mut writer = storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    path("drop")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged,
                ),
                Default::default(),
            )
            .await?;
        writer.write_chunk(b"12345678").await?;
        drop(writer.commit());
    }
    assert_eq!(storage.used_bytes(), 0);
    assert!(!exists(&storage, "abort").await?);
    assert!(!exists(&storage, "drop").await?);
    put(&storage, "empty", b"").await?;
    assert!(get(&storage, "empty").await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn ranges_chunking_and_repeated_eof_follow_the_stream_contract() -> TestResult {
    let storage = storage(200_000)?;
    let bytes: Vec<u8> = (0..150_000).map(|index| (index % 251) as u8).collect();
    put(&storage, "file", &bytes).await?;
    for (offset, length, expected) in [
        (0, None, bytes.as_slice()),
        (65_535, Some(70_000), &bytes[65_535..135_535]),
        (149_999, Some(u64::MAX - 149_999), &bytes[149_999..]),
        (u64::MAX, None, &[]),
        (1, Some(0), &[]),
    ] {
        let mut reader = storage
            .read_file(
                ah_vfs_storage::ReadRequest::new(
                    path("file")?.to_rooted_expr(),
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
        assert_eq!(actual, expected);
    }
    Ok(())
}

#[tokio::test]
async fn quota_counts_staging_and_failure_discards_the_stage() -> TestResult {
    let storage = storage(8)?;
    let mut one = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("one")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    let mut two = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("two")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    one.write_chunk(b"12345").await?;
    assert_eq!(
        two.write_chunk(b"6789")
            .await
            .err()
            .ok_or("expected full")?
            .failure(),
        Failure::Backend(BackendFailure::CapacityExceeded)
    );
    assert_eq!(storage.used_bytes(), 5);
    assert!(
        matches!(two.write_chunk(b"678").await, Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed))
    );
    assert!(
        matches!(two.commit().await, Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed))
    );
    let mut two = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("two")?.to_rooted_expr(),
                WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    two.write_chunk(b"678").await?;
    assert_eq!(storage.used_bytes(), 8);
    let failure = one.write_chunk(b"x").await.err().ok_or("expected full")?;
    assert_eq!(
        failure.failure(),
        Failure::Backend(BackendFailure::CapacityExceeded)
    );
    assert_eq!(
        failure.outcome(),
        ah_vfs_storage::MutationOutcome::Unchanged
    );
    assert!(
        matches!(one.commit().await, Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed))
    );
    assert!(!exists(&storage, "one").await?);
    two.commit().await?;
    assert_eq!(storage.used_bytes(), 3);
    Ok(())
}

#[tokio::test]
async fn readers_pin_old_versions_and_their_capacity_until_released() -> TestResult {
    let storage = storage(12)?;
    put(&storage, "file", b"oldold").await?;
    let mut old = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                path("file")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("file")?.to_rooted_expr(),
                WriteMode::Replace,
                ah_vfs_storage::Publication::Atomic,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"newnew").await?;
    assert_eq!(get(&storage, "file").await?, b"oldold");
    writer.commit().await?;
    assert_eq!(storage.used_bytes(), 12);
    assert_eq!(get(&storage, "file").await?, b"newnew");
    assert_eq!(collect(old.as_mut()).await?, b"oldold");
    assert_eq!(storage.used_bytes(), 6);
    let reader = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                path("file")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    storage
        .remove_entry(StorageRemoveEntryRequest {
            path: path("file")?,
        })
        .await?;
    assert_eq!(storage.used_bytes(), 6);
    drop(reader);
    assert_eq!(storage.used_bytes(), 0);
    Ok(())
}

#[tokio::test]
async fn full_capacity_replacement_requires_staging_room_and_keeps_original_on_abort() -> TestResult
{
    let storage = storage(4)?;
    put(&storage, "file", b"full").await?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("file")?.to_rooted_expr(),
                WriteMode::Replace,
                ah_vfs_storage::Publication::Atomic,
            ),
            Default::default(),
        )
        .await?;
    assert_eq!(
        writer
            .write_chunk(b"new")
            .await
            .err()
            .ok_or("expected full")?
            .failure(),
        Failure::Backend(BackendFailure::CapacityExceeded)
    );
    assert!(
        matches!(writer.abort().await, Err(error) if error.failure() == Failure::Backend(BackendFailure::Closed))
    );
    assert_eq!(get(&storage, "file").await?, b"full");
    assert_eq!(storage.used_bytes(), 4);
    Ok(())
}

#[tokio::test]
async fn directories_and_unicode_name_pagination_remain_hierarchical() -> TestResult {
    let storage = storage(64)?;
    mkdir(&storage, "dir/nested").await?;
    mkdir(&storage, "dir/nested").await?;
    for name in ["z", "a", "é", "文档"] {
        put(&storage, &format!("dir/{name}"), b"x").await?;
    }
    put(&storage, "dir/nested/hidden", b"x").await?;
    let mut names = Vec::new();
    let mut target = ah_vfs_storage::ListTarget::Path(path("dir")?.to_rooted_expr());
    loop {
        let query = ah_vfs_storage::ListRequest::new(target, ah_vfs_storage::PageSize::new(2)?);
        let page = storage.list_entries(&query, Default::default()).await?;
        names.extend(page.entries().iter().map(|entry| {
            entry
                .path()
                .name()
                .map(|name| name.as_str().to_owned())
                .unwrap_or_default()
        }));
        let Some(next) = page.continuation() else {
            break;
        };
        target = ah_vfs_storage::ListTarget::Continue(next);
    }
    assert_eq!(names, ["a", "nested", "z", "é", "文档"]);
    assert_eq!(
        storage
            .remove_entry(StorageRemoveEntryRequest { path: path("dir")? })
            .await
            .err()
            .ok_or("not empty")?
            .failure(),
        Failure::Rejected(Rejection::DirectoryNotEmpty)
    );
    for after in ["a/b", "..", "", "a:b"] {
        assert!(ah_vfs_storage::EntryName::parse(after).is_err());
    }
    assert!(ah_vfs_storage::PageSize::new(1001).is_err());
    Ok(())
}

#[tokio::test]
async fn invalid_parents_types_and_root_mutations_have_domain_errors() -> TestResult {
    let storage = storage(64)?;
    put(&storage, "file", b"data").await?;
    assert_eq!(
        storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    path("missing/file")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged
                ),
                Default::default()
            )
            .await
            .err()
            .ok_or("parent")?
            .failure(),
        Failure::Rejected(Rejection::NotFound)
    );
    assert_eq!(
        storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    path("file/child")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged
                ),
                Default::default()
            )
            .await
            .err()
            .ok_or("parent")?
            .failure(),
        Failure::Rejected(Rejection::NotDirectory)
    );
    assert_eq!(
        storage
            .stat_entry(
                ah_vfs_storage::StatRequest::new(
                    path("file/child")?.to_rooted_expr(),
                    ah_vfs_storage::SymlinkAction::Preserve
                ),
                Default::default()
            )
            .await
            .err()
            .ok_or("parent")?
            .failure(),
        Failure::Rejected(Rejection::NotDirectory)
    );
    assert_eq!(
        storage
            .read_file(
                ah_vfs_storage::ReadRequest::new(
                    path("")?.to_rooted_expr(),
                    ah_vfs_storage::ReadRange::FULL
                ),
                Default::default()
            )
            .await
            .err()
            .ok_or("dir")?
            .failure(),
        Failure::Rejected(Rejection::IsDirectory)
    );
    assert_eq!(
        storage
            .list_entries(
                &ah_vfs_storage::ListRequest::new(
                    ah_vfs_storage::ListTarget::Path(path("file")?.to_rooted_expr()),
                    Default::default()
                ),
                Default::default()
            )
            .await
            .err()
            .ok_or("file")?
            .failure(),
        Failure::Rejected(Rejection::NotDirectory)
    );
    assert_eq!(
        storage
            .remove_entry(StorageRemoveEntryRequest { path: path("")? })
            .await
            .err()
            .ok_or("root")?
            .failure(),
        Failure::Rejected(Rejection::RootProtected)
    );
    assert_eq!(
        storage
            .write_file(
                ah_vfs_storage::WriteRequest::new(
                    path("")?.to_rooted_expr(),
                    ah_vfs_storage::WriteMode::CreateNew,
                    ah_vfs_storage::Publication::Staged
                ),
                Default::default()
            )
            .await
            .err()
            .ok_or("root")?
            .failure(),
        Failure::Rejected(Rejection::RootProtected)
    );
    assert_eq!(
        storage
            .create_dir(
                ah_vfs_storage::CreateDirectoryRequest::new(
                    path("file/nested/dir")?.to_rooted_expr(),
                    ah_vfs_storage::CreateDirectoryMode::Recursive
                ),
                Default::default()
            )
            .await
            .err()
            .ok_or("parent")?
            .failure(),
        Failure::Rejected(Rejection::NotDirectory)
    );
    assert!(!exists(&storage, "nested").await?);
    Ok(())
}

#[tokio::test]
async fn subtree_rename_and_atomic_replacement_preserve_contents() -> TestResult {
    let storage = storage(64)?;
    mkdir(&storage, "source/nested").await?;
    mkdir(&storage, "target").await?;
    put(&storage, "source/nested/file", b"data").await?;
    storage
        .move_entry(StorageMoveEntryRequest {
            source: path("source")?,
            destination: path("target")?,
            replace: true,
        })
        .await?;
    assert!(!exists(&storage, "source").await?);
    assert_eq!(get(&storage, "target/nested/file").await?, b"data");
    put(&storage, "old", b"old").await?;
    put(&storage, "new", b"new").await?;
    assert_eq!(
        storage
            .move_entry(StorageMoveEntryRequest {
                source: path("new")?,
                destination: path("old")?,
                replace: false
            })
            .await
            .err()
            .ok_or("exists")?
            .failure(),
        Failure::Rejected(Rejection::AlreadyExists)
    );
    storage
        .move_entry(StorageMoveEntryRequest {
            source: path("new")?,
            destination: path("old")?,
            replace: true,
        })
        .await?;
    assert_eq!(get(&storage, "old").await?, b"new");
    assert!(!exists(&storage, "new").await?);
    assert_eq!(storage.used_bytes(), 7);
    Ok(())
}

#[tokio::test]
async fn failed_renames_leave_every_entry_and_quota_unchanged() -> TestResult {
    let storage = storage(64)?;
    mkdir(&storage, "dir/sub").await?;
    mkdir(&storage, "other/sub").await?;
    put(&storage, "file", b"data").await?;
    for (source, destination, expected) in [
        ("dir", "dir/sub/new", Failure::Rejected(Rejection::Conflict)),
        (
            "dir",
            "other",
            Failure::Rejected(Rejection::DirectoryNotEmpty),
        ),
        ("dir", "file", Failure::Rejected(Rejection::NotDirectory)),
        ("file", "dir", Failure::Rejected(Rejection::IsDirectory)),
        ("", "new", Failure::Rejected(Rejection::RootProtected)),
        ("file", "", Failure::Rejected(Rejection::RootProtected)),
        ("missing", "new", Failure::Rejected(Rejection::NotFound)),
    ] {
        assert_eq!(
            storage
                .move_entry(StorageMoveEntryRequest {
                    source: path(source)?,
                    destination: path(destination)?,
                    replace: true
                })
                .await
                .err()
                .ok_or("expected failure")?
                .failure(),
            expected
        );
    }
    assert_eq!(get(&storage, "file").await?, b"data");
    assert!(exists(&storage, "dir/sub").await?);
    assert!(exists(&storage, "other/sub").await?);
    assert_eq!(storage.used_bytes(), 4);
    let long = "x".repeat(4094);
    assert_eq!(
        storage
            .move_entry(StorageMoveEntryRequest {
                source: path("dir")?,
                destination: path(&long)?,
                replace: false
            })
            .await
            .err()
            .ok_or("too long")?
            .failure(),
        Failure::Rejected(Rejection::PathTooLong)
    );
    assert!(exists(&storage, "dir/sub").await?);
    Ok(())
}

#[tokio::test]
async fn changed_parent_rejects_publication_and_failed_commit_is_terminal() -> TestResult {
    let storage = storage(8)?;
    mkdir(&storage, "dir").await?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("dir/file")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"data").await?;
    storage
        .remove_entry(StorageRemoveEntryRequest { path: path("dir")? })
        .await?;
    mkdir(&storage, "dir").await?;
    assert_eq!(
        writer
            .commit()
            .await
            .err()
            .ok_or("parent changed")?
            .failure(),
        Failure::Rejected(Rejection::Conflict)
    );
    assert_eq!(
        writer.commit().await.err().ok_or("closed")?.failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    assert_eq!(storage.used_bytes(), 0);
    assert!(!exists(&storage, "dir/file").await?);
    Ok(())
}

#[tokio::test]
async fn concurrent_create_new_publication_has_one_winner_and_refunds_loser() -> TestResult {
    let storage = storage(16)?;
    let mut one = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("file")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    let mut two = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("file")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    one.write_chunk(b"one").await?;
    two.write_chunk(b"two").await?;
    let (first, second) = tokio::join!(one.commit(), two.commit());
    assert!(first.is_ok());
    assert_eq!(
        second.err().ok_or("conflict")?.failure(),
        Failure::Rejected(Rejection::Conflict)
    );
    assert_eq!(storage.used_bytes(), 3);
    assert_eq!(get(&storage, "file").await?, b"one");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writers_cannot_overcommit_instance_quota() -> TestResult {
    let storage = Arc::new(storage(16)?);
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for index in 0..8 {
        let storage = storage.clone();
        let barrier = barrier.clone();
        let file = path(&format!("file{index}"))?;
        tasks.push(tokio::spawn(async move {
            let mut writer = storage
                .write_file(
                    ah_vfs_storage::WriteRequest::new(
                        file.to_rooted_expr(),
                        ah_vfs_storage::WriteMode::CreateNew,
                        ah_vfs_storage::Publication::Staged,
                    ),
                    Default::default(),
                )
                .await?;
            barrier.wait().await;
            writer.write_chunk(b"1234").await?;
            writer.commit().await?;
            Ok::<_, ah_vfs_storage::StorageError>(())
        }));
    }
    let mut success = 0;
    for task in tasks {
        match task.await? {
            Ok(()) => success += 1,
            Err(error) => assert_eq!(
                error.failure(),
                Failure::Backend(BackendFailure::CapacityExceeded)
            ),
        }
    }
    assert_eq!(success, 4);
    assert_eq!(storage.used_bytes(), 16);
    Ok(())
}

#[tokio::test]
async fn streams_keep_resources_alive_after_the_storage_handle_is_dropped() -> TestResult {
    let storage = storage(16)?;
    put(&storage, "existing", b"retained").await?;
    let mut reader = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                path("existing")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                path("new")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    drop(storage);
    assert_eq!(collect(reader.as_mut()).await?, b"retained");
    writer.write_chunk(b"new").await?;
    assert_eq!(
        writer.commit().await?.entry_type(),
        EntryType::file(Some(3))
    );
    Ok(())
}
