#![cfg(target_os = "linux")]
//! End-to-end checks use the actual native plugin executable and a TCP channel.
use ah_vfs::application::driver_error::LifecycleFailure;
use ah_vfs_storage::{
    BackendFailure, Failure, MutationOutcome, Operation, PathExpr,
    ReadLinkRequest, Rejection,
};
mod common;
use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::dto::*;
use ah_vfs::application::port::*;
use ah_vfs_storage::StoragePath;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::Arc,
};
use vfs_plugin_grpc::GrpcTransport;
use vfs_plugin_host::PluginDriver;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Process {
    child: Child,
    endpoint: String,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Process {
    fn start(address: &str) -> Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_vfs-driver-local-linux"))
            .arg(address)
            .env("VFS_PLUGIN_TOKEN", "isolated-test-token")
            .stdout(Stdio::piped())
            .spawn()?;
        let mut line = String::new();
        BufReader::new(child.stdout.take().ok_or("stdout")?).read_line(&mut line)?;
        if line.trim().is_empty() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("plugin failed to start".into());
        }
        Ok(Self {
            child,
            endpoint: format!("http://{}", line.trim()),
        })
    }
    async fn driver(&self) -> Result<Arc<PluginDriver>> {
        Ok(Arc::new(
            PluginDriver::connect(Arc::new(
                GrpcTransport::connect(self.endpoint.clone(), "isolated-test-token").await?,
            ))
            .await?,
        ))
    }
}
async fn storage(
    driver: &dyn Driver,
    root: &std::path::Path,
    records: sqlx::SqlitePool,
) -> Result<Arc<dyn Storage>> {
    let registry = common::registry(driver, records).await?;
    let instance = registry.create(common::local_config(root)?).await?;
    Ok(driver
        .open(registry.open_context(instance.id).await?)
        .await?)
}

#[tokio::test]
async fn slot_conflict_preserves_the_generation_and_disconnect_invalidates_it() -> Result {
    let records = common::pool().await?;
    let mut process = Process::start("127.0.0.1:0")?;
    let driver = process.driver().await?;
    let mut catalog = DriverCatalog::new();
    catalog.register(driver.clone(), Default::default()).await?;
    let root = tempfile::tempdir()?;
    let storage = storage(driver.as_ref(), root.path(), records).await?;
    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                StoragePath::parse("occupied")?.to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"private").await?;
    std::fs::write(root.path().join("occupied"), b"other writer")?;
    let error = writer
        .commit()
        .await
        .err()
        .ok_or("occupied slot was overwritten")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::Conflict));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert_eq!(std::fs::read(root.path().join("occupied"))?, b"other writer");

    let stat = ah_vfs_storage::StatRequest::new(
        StoragePath::root().to_rooted_expr(),
        ah_vfs_storage::SymlinkAction::Preserve,
    );
    storage.stat_entry(stat.clone(), Default::default()).await?;
    process.child.kill()?;
    process.child.wait()?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !storage.is_stale() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await?;
    let error = storage
        .stat_entry(stat, Default::default())
        .await
        .err()
        .ok_or("retired generation accepted a request")?;
    assert_eq!(error.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    Ok(())
}

#[tokio::test]
async fn bound_link_rechecks_the_original_slot_over_grpc() -> Result {
    let records = common::pool().await?;
    let mut process = Process::start("127.0.0.1:0")?;
    let driver = process.driver().await?;
    let mut catalog = DriverCatalog::new();
    catalog.register(driver.clone(), Default::default()).await?;
    let root = tempfile::tempdir()?;
    std::os::unix::fs::symlink("before", root.path().join("link"))?;
    let storage = storage(driver.as_ref(), root.path(), records).await?;
    let operand = PathExpr::parse("/link")?;
    let old =
        ah_vfs::resolve::bind_link(storage.as_ref(), operand.clone(), Default::default()).await?;
    let request = ReadLinkRequest::new(StoragePath::parse("link")?);
    assert_eq!(old.read(request.clone()).await?.as_bytes(), b"before");
    let wrong = ReadLinkRequest::new(StoragePath::parse("other")?);
    let error = old
        .read(wrong.clone())
        .await
        .err()
        .ok_or("accepted a different address")?;
    assert_eq!(error.failure(), Failure::InvalidBinding);
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);

    let mut lookup = storage.begin_lookup(Operation::ReadLink).await?;
    let parent = lookup.root().await?;
    let name = ah_vfs_storage::EntryName::parse("link")?;
    let link = lookup.observe(&parent, &name).await?;
    let error = lookup
        .execute_read_link(&parent, &name, &link, wrong)
        .await
        .err()
        .ok_or("backend accepted a different address")?;
    assert_eq!(error.failure(), Failure::InvalidBinding);
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert_eq!(
        lookup
            .execute_read_link(&parent, &name, &link, request.clone())
            .await?
            .as_bytes(),
        b"before"
    );

    std::fs::rename(root.path().join("link"), root.path().join("original"))?;
    std::os::unix::fs::symlink("after", root.path().join("link"))?;
    let error = old.read(request.clone()).await.err().ok_or("accepted the replaced link")?;
    assert_eq!(error.failure(), Failure::Rejected(Rejection::Conflict));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    let current = ah_vfs::resolve::bind_link(storage.as_ref(), operand, Default::default()).await?;
    assert_eq!(current.read(request.clone()).await?.as_bytes(), b"after");

    process.child.kill()?;
    process.child.wait()?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !storage.is_stale() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }).await?;
    let error = current.read(request).await.err().ok_or("accepted a retired binding")?;
    assert_eq!(error.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    Ok(())
}

#[tokio::test]
async fn native_process_passes_shared_storage_contracts_over_grpc() -> Result {
    let records = common::pool().await?;
    let process = Process::start("127.0.0.1:0")?;
    let driver = process.driver().await?;
    let mut catalog = DriverCatalog::new();
    catalog.register(driver.clone(), Default::default()).await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_range_read_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_create_dir_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_pagination_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    ah_vfs::testing::assert_same_path_rename_contract(
        storage(driver.as_ref(), root.path(), records.clone())
            .await?
            .as_ref(),
        driver.info().capabilities().rename(),
    )
    .await?;
    let root = tempfile::tempdir()?;
    let storage = storage(driver.as_ref(), root.path(), records.clone()).await?;
    use ah_vfs_storage::{ListRequest, ListTarget, PageSize};
    std::fs::create_dir_all(root.path().join("docs/a"))?;
    std::fs::create_dir(root.path().join("docs/b"))?;
    let list = ListRequest::new(
        ListTarget::Path(StoragePath::parse("docs")?.to_rooted_expr()),
        PageSize::new(1)?,
    );
    let page = storage.list_entries(&list, Default::default()).await?;
    let resume = ListRequest::new(
        ListTarget::Continue(page.continuation().ok_or("expected continuation")?),
        PageSize::DEFAULT,
    );
    std::fs::remove_dir_all(root.path().join("docs"))?;
    std::fs::create_dir(root.path().join("docs"))?;
    assert!(
        matches!(storage.list_entries(&resume, Default::default()).await,
        Err(error) if error.failure() == Failure::Backend(BackendFailure::Stale))
    );
    assert!(!storage.is_stale());
    assert!(
        storage
            .list_entries(&list, Default::default())
            .await?
            .entries()
            .is_empty()
    );
    use ah_vfs_storage::{
        AttributeAction as A, AttributeRequest, AttributeResult as R, IntermediateSymlinks,
        PathExpr, Permissions, Rejection, XattrName, XattrWriteMode,
    };
    use std::num::NonZeroUsize;
    std::fs::write(root.path().join("docs/attributes"), b"test")?;
    std::os::unix::fs::symlink("/docs", root.path().join("alias"))?;
    let attributes = storage.attributes().ok_or("missing attributes port")?;
    let execute = |action| -> Result<_> {
        Ok(attributes.execute(
            AttributeRequest::new(PathExpr::parse("../alias/attributes")?, action),
            LookupContext {
                working_directory: StoragePath::parse("docs")?,
                intermediate_symlinks: IntermediateSymlinks::FollowWithinInstance,
            },
        ))
    };
    let mode = Permissions::new(0o640)?;
    assert_eq!(execute(A::SetPermissions(mode))?.await?, R::Unit);
    assert_eq!(execute(A::GetPermissions)?.await?, R::Permissions(mode));
    let name = XattrName::parse("user.grpc")?;
    let bound = NonZeroUsize::new(1024).ok_or("limit")?;
    let set = execute(A::set_xattr(
        name.clone(),
        vec![0, 255, 0],
        XattrWriteMode::CreateNew,
    ))?
    .await;
    if !matches!(&set, Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported)) {
        assert_eq!(set?, R::Unit);
        assert_eq!(
            execute(A::get_xattr(name.clone(), bound))?.await?,
            R::Value(vec![0, 255, 0])
        );
        let R::Names(names) = execute(A::list_xattrs(bound))?.await? else {
            return Err("missing name list".into());
        };
        assert_eq!(names.names(), &[name.clone()]);
        assert_eq!(execute(A::RemoveXattr(name))?.await?, R::Unit);
    }

    let mut writer = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                (StoragePath::parse("retained")?).to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    writer.write_chunk(b"independent session").await?;
    let key = driver.info().key().clone();
    catalog.unregister(&key)?;
    drop(driver);
    drop(catalog);
    writer.commit().await?;
    let mut reader = storage
        .read_file(
            ah_vfs_storage::ReadRequest::new(
                StoragePath::parse("retained")?.to_rooted_expr(),
                ah_vfs_storage::ReadRange::FULL,
            ),
            Default::default(),
        )
        .await?;
    drop(storage);
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await? {
        bytes.extend(chunk);
    }
    assert_eq!(bytes, b"independent session");
    assert!(reader.next_chunk().await?.is_none());
    let denied = PluginDriver::connect(Arc::new(
        GrpcTransport::connect(process.endpoint.clone(), "wrong-token").await?,
    ))
    .await
    .err()
    .ok_or("authentication must fail")?;
    assert_eq!(
        denied.lifecycle_error().ok_or("category")?.failure(),
        LifecycleFailure::AccessDenied
    );
    Ok(())
}
#[tokio::test]
async fn process_restart_rejects_old_resources_without_replaying_commit() -> Result {
    let records = common::pool().await?;
    let mut process = Process::start("127.0.0.1:0")?;
    let address = process
        .endpoint
        .strip_prefix("http://")
        .ok_or("address")?
        .to_owned();
    let driver = process.driver().await?;
    let mut catalog = DriverCatalog::new();
    catalog.register(driver.clone(), Default::default()).await?;
    let root = tempfile::tempdir()?;
    let storage = storage(driver.as_ref(), root.path(), records.clone()).await?;
    let mut pending = storage
        .write_file(
            ah_vfs_storage::WriteRequest::new(
                (StoragePath::parse("uncertain")?).to_rooted_expr(),
                ah_vfs_storage::WriteMode::CreateNew,
                ah_vfs_storage::Publication::Staged,
            ),
            Default::default(),
        )
        .await?;
    pending.write_chunk(b"unpublished").await?;
    process.child.kill()?;
    process.child.wait()?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !storage.is_stale() {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await?;
    let error = pending.commit().await.err().ok_or("commit must fail")?;
    assert_eq!(error.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(error.operation(), Operation::Write);
    assert_eq!(error.outcome(), MutationOutcome::Unchanged);
    assert_eq!(
        pending.commit().await.err().ok_or("terminal")?.failure(),
        Failure::Backend(BackendFailure::Closed)
    );
    assert!(!root.path().join("uncertain").exists());
    let restarted = Process::start(&address)?;
    let fresh = restarted.driver().await?;
    let mut fresh_catalog = DriverCatalog::new();
    fresh_catalog
        .register(fresh.clone(), Default::default())
        .await?;
    let old = storage
        .stat_entry(
            ah_vfs_storage::StatRequest::new(
                (StoragePath::root()).to_rooted_expr(),
                ah_vfs_storage::SymlinkAction::Preserve,
            ),
            Default::default(),
        )
        .await
        .err()
        .ok_or("old generation must fail")?;
    assert_eq!(old.failure(), Failure::Backend(BackendFailure::Stale));
    assert_eq!(
        common::registry(fresh.as_ref(), records.clone())
            .await?
            .list(InstanceQuery::default())
            .await?
            .instances
            .len(),
        1
    );
    Ok(())
}
