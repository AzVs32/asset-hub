use ah_vfs::application::driver_error::{LifecycleError, LifecycleFailure};
use ah_vfs::application::mount_runtime::MountRuntime;
use ah_vfs_storage::{
    BackendFailure, ContractViolation, Failure, Operation, Rejection, StorageError,
};
mod common;

use ah_vfs::application::driver_catalog::DriverCatalog;
use ah_vfs::application::driver_error::DriverError;
use ah_vfs::application::driver_info::DriverInfo;
use ah_vfs::application::dto::{CreateDirRequest, ReadFileRequest, WriteFileRequest};
use ah_vfs::application::error::EntryError;
use ah_vfs::application::port::{
    DriverFuture, LookupScope, ObservedEntry, OperationLookup, Storage, StorageAttributes,
    StorageFileReader, StorageFileWriter, StorageFuture, validate_storage_extensions,
};
use ah_vfs::application::service::{EntryService, MountService};
use ah_vfs::domain::namespace::{ChildMountPolicy, VirtualPath};
use ah_vfs::domain::{DriverInstance, DriverKey, InstanceId};
use ah_vfs::infrastructure::mount_repository::SqliteMountRepository;
use ah_vfs_storage::{
    AttributeAction, AttributeRequest, AttributeResult, Entry, EntryName, EntryType, LinkTarget,
    Permissions, StoragePath, XattrName,
};
use ah_vfs_storage::{
    AttributeSupport, Capabilities, MutationBinding, ReadRange, ReadSupport, WriteMode, WriteModes,
    WriteSupport,
};
use std::{
    error::Error,
    io,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
type TestResult = Result<(), Box<dyn Error>>;

fn failure(operation: Operation, kind: Failure) -> StorageError {
    StorageError::without_effects(
        operation,
        kind,
        Some(Box::new(io::Error::other("backend-private-token"))),
    )
}
struct TestReader;
impl StorageFileReader for TestReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async {
            Err(failure(
                Operation::Read,
                Failure::Backend(BackendFailure::Stale),
            ))
        })
    }
}
struct TestWriter {
    closed: bool,
}
impl StorageFileWriter for TestWriter {
    fn write_chunk<'a>(&'a mut self, _: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async {
            if self.closed {
                Err(StorageError::without_effects(
                    Operation::Write,
                    Failure::Backend(BackendFailure::Closed),
                    None,
                ))
            } else {
                Ok(())
            }
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, Entry> {
        Box::pin(async {
            if self.closed {
                return Err(StorageError::without_effects(
                    Operation::Write,
                    Failure::Backend(BackendFailure::Closed),
                    None,
                ));
            }
            self.closed = true;
            Err(StorageError::after_dispatch(
                Operation::Write,
                Failure::Backend(BackendFailure::Other),
                Some(Box::new(io::Error::other("backend-private-token"))),
            ))
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async {
            if self.closed {
                return Err(StorageError::without_effects(
                    Operation::Write,
                    Failure::Backend(BackendFailure::Closed),
                    None,
                ));
            }
            self.closed = true;
            Ok(())
        })
    }
}
struct TestStorage {
    calls: Arc<AtomicUsize>,
}
impl Storage for TestStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn read_file(
        &self,
        _: ah_vfs_storage::ReadRequest,
        _: ah_vfs::application::port::LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Box::new(TestReader) as Box<dyn StorageFileReader>) })
    }
    fn write_file(
        &self,
        _: ah_vfs_storage::WriteRequest,
        _: ah_vfs::application::port::LookupContext,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Box::new(TestWriter { closed: false }) as Box<dyn StorageFileWriter>) })
    }
}
struct TestDriver {
    info: DriverInfo,
    storage: Arc<dyn Storage>,
    open_failure: Option<LifecycleFailure>,
}
impl common::Backend for TestDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn open(
        &self,
        context: ah_vfs::application::port::OpenContext,
    ) -> DriverFuture<'_, Arc<dyn Storage>> {
        let _ = context.instance_id().clone();
        Box::pin(async {
            if let Some(kind) = self.open_failure {
                return Err(LifecycleError::with_source(
                    kind,
                    io::Error::other("backend-private-token"),
                )
                .into());
            }
            Ok(self.storage.clone())
        })
    }
}
async fn fixture(
    declared: Capabilities,
    open_failure: Option<LifecycleFailure>,
) -> Result<(EntryService, Arc<AtomicUsize>), Box<dyn Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let storage = Arc::new(TestStorage {
        calls: calls.clone(),
    });
    let service = entry_fixture(declared, open_failure, storage).await?;
    Ok((service, calls))
}

async fn entry_fixture(
    declared: Capabilities,
    open_failure: Option<LifecycleFailure>,
    storage: Arc<dyn Storage>,
) -> Result<EntryService, Box<dyn Error>> {
    let key = DriverKey::parse("test")?;
    let mut drivers = DriverCatalog::new();
    common::register(
        &mut drivers,
        common::managed(Arc::new(TestDriver {
            info: DriverInfo::builder()
                .key(key.clone())
                .name("Test".into())
                .version(semver::Version::new(0, 1, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .capabilities(declared)
                .build(),
            storage,
            open_failure,
        })),
    )?;
    let drivers = Arc::new(drivers);
    let repository = Arc::new(SqliteMountRepository::in_memory().await?);
    let runtime = Arc::new({
        let drivers = drivers;
        let records = common::Records::seeded(&drivers, &["root"])?;
        MountRuntime::new(repository, drivers, records, None)
    });
    MountService::new(runtime.clone())
        .initialize_root(DriverInstance::new(key, InstanceId::parse("root")?))
        .await?;
    Ok(EntryService::new(runtime))
}
fn request(atomic: bool) -> Result<WriteFileRequest, Box<dyn Error>> {
    Ok(WriteFileRequest {
        path: VirtualPath::parse("/file")?,
        mode: WriteMode::CreateNew,
        publication: if atomic {
            ah_vfs_storage::Publication::Atomic
        } else {
            ah_vfs_storage::Publication::Staged
        },
    })
}

#[test]
fn normalized_errors_preserve_sources_without_exposing_backend_messages() -> TestResult {
    for kind in [
        Failure::Rejected(Rejection::NotFound),
        Failure::Backend(BackendFailure::Stale),
        Failure::Backend(BackendFailure::RateLimited),
        Failure::Rejected(Rejection::Conflict),
        Failure::Backend(BackendFailure::TimedOut),
        Failure::Backend(BackendFailure::Other),
    ] {
        let error = failure(Operation::Stat, kind);
        assert_eq!(error.failure(), kind);
        assert!(!error.to_string().contains("backend-private-token"));
        assert_eq!(
            error.source().ok_or("missing source")?.to_string(),
            "backend-private-token"
        );
        let entry = EntryError::from(DriverError::from(error));
        assert_eq!(
            entry
                .storage_error()
                .ok_or("missing normalized error")?
                .failure(),
            kind
        );
        assert!(!entry.to_string().contains("backend-private-token"));
    }
    Ok(())
}

#[test]
fn attribute_values_reject_invalid_input_without_normalizing_identity() -> TestResult {
    assert_eq!(Permissions::new(0o7640)?.bits(), 0o7640);
    assert!(Permissions::new(0o100644).is_err());
    for name in ["", "bad\0name", &"a".repeat(256)] {
        assert!(XattrName::parse(name).is_err());
    }
    assert_eq!(XattrName::parse("user.文档")?.as_str(), "user.文档");
    Ok(())
}

#[tokio::test]
async fn unsupported_requests_are_rejected_before_backend_side_effects() -> TestResult {
    let caps = Capabilities::default()
        .with_read(ReadSupport::Sequential)
        .with_write(WriteSupport::staged(WriteModes::CreateNew, false));
    let (unavailable, _) = fixture(caps, Some(LifecycleFailure::Unavailable)).await?;
    assert!(
        matches!(unavailable.write_file(request(true)?).await, Err(EntryError::Storage(error))
        if error.failure() == Failure::Rejected(Rejection::Unsupported))
    );
    let create = CreateDirRequest::new(
        "/absent/child",
        ah_vfs_storage::CreateDirectoryMode::Recursive,
    )?;
    assert!(
        matches!(unavailable.create_dir(create).await, Err(EntryError::Storage(error))
        if error.operation() == Operation::CreateDirectory
            && error.failure() == Failure::Rejected(Rejection::Unsupported))
    );
    let (service, calls) = fixture(caps, None).await?;
    assert!(
        matches!(service.write_file(request(true)?).await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::Unsupported)))
    );
    let read = ReadFileRequest {
        path: VirtualPath::parse("/file")?,
        range: ReadRange::new(1, None)?,
    };
    assert!(matches!(service.read_file(read).await, Err(error)
        if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::Unsupported))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut writer = service.write_file(request(false)?).await?;
    writer.abort().await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn sequential_reads_admit_a_bounded_prefix_before_opening_the_backend() -> TestResult {
    let capabilities = Capabilities::default().with_read(ReadSupport::Sequential);
    let (unavailable, _) = fixture(capabilities, Some(LifecycleFailure::Unavailable)).await?;
    let path = VirtualPath::parse("/file")?;
    assert!(matches!(
        unavailable.read_file(ReadFileRequest {
            path: path.clone(),
            range: ReadRange::new(1, Some(1))?,
        }).await,
        Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Rejected(Rejection::Unsupported))
    ));
    let (service, calls) = fixture(capabilities, None).await?;
    let _reader = service
        .read_file(ReadFileRequest {
            path,
            range: ReadRange::new(0, Some(1))?,
        })
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn invalid_routed_suffix_is_rejected_before_backend_open_or_io() -> TestResult {
    // An open failure would be observed if path conversion happened too late.
    let (service, calls) =
        fixture(Capabilities::default(), Some(LifecycleFailure::Unavailable)).await?;
    let error = service
        .read_file(ReadFileRequest {
            path: VirtualPath::parse("/file:stream")?,
            range: ReadRange::FULL,
        })
        .await
        .err()
        .ok_or("expected invalid storage path")?;
    assert!(matches!(
        error,
        ah_vfs::application::error::EntryError::InvalidStorageAddress(
            ah_vfs_storage::PathError::InvalidComponent(_)
        )
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn opening_streaming_and_committing_keep_domain_error_categories() -> TestResult {
    let caps = Capabilities::default()
        .with_read(ReadSupport::Range)
        .with_write(WriteSupport::staged(WriteModes::CreateNew, false));
    let (service, _) = fixture(caps, Some(LifecycleFailure::Unavailable)).await?;
    let path = VirtualPath::parse("/file")?;
    let read = ReadFileRequest {
        path: path.clone(),
        range: ReadRange::FULL,
    };
    assert!(
        matches!(service.read_file(read).await, Err(EntryError::Driver(DriverError::Lifecycle(error)))
        if error.failure() == LifecycleFailure::Unavailable)
    );
    let (service, _) = fixture(caps, None).await?;
    let mut reader = service
        .read_file(ReadFileRequest {
            path,
            range: ReadRange::FULL,
        })
        .await?;
    assert!(
        matches!(reader.next_chunk().await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Backend(BackendFailure::Stale)))
    );
    let mut writer = service.write_file(request(false)?).await?;
    assert!(
        matches!(writer.commit().await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Backend(BackendFailure::Other)))
    );
    assert!(
        matches!(writer.commit().await, Err(error) if error.storage_error().is_some_and(|e| e.failure() == Failure::Backend(BackendFailure::Closed)))
    );
    Ok(())
}

struct ReadOnlyPermissions;
impl StorageAttributes for ReadOnlyPermissions {
    fn support(&self) -> (AttributeSupport, AttributeSupport) {
        (AttributeSupport::Read, AttributeSupport::None)
    }
}

struct PermissionStorage(ReadOnlyPermissions);
impl Storage for PermissionStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn begin_lookup(&self, op: Operation) -> StorageFuture<'_, Box<dyn OperationLookup>> {
        Box::pin(async move {
            if op != Operation::GetPermissions {
                return Err(failure(op, Failure::Rejected(Rejection::Unsupported)));
            }
            Ok(Box::new(PermissionLookup(LookupScope::default())) as Box<dyn OperationLookup>)
        })
    }
    fn attributes(&self) -> Option<&dyn StorageAttributes> {
        Some(&self.0)
    }
}

struct PermissionLookup(LookupScope);

impl PermissionLookup {
    fn bind(&self, path: StoragePath, kind: EntryType) -> Result<ObservedEntry, StorageError> {
        let entry = Entry::new(path, kind, None).map_err(|_| {
            failure(
                Operation::GetPermissions,
                Failure::Contract(ContractViolation::InvalidEntry),
            )
        })?;
        Ok(self.0.bind(entry, ()))
    }
}

impl OperationLookup for PermissionLookup {
    fn operation(&self) -> Operation {
        Operation::GetPermissions
    }

    fn root(&mut self) -> StorageFuture<'_, ObservedEntry> {
        Box::pin(async move { self.bind(StoragePath::root(), EntryType::Directory) })
    }

    fn observe<'a>(
        &'a mut self,
        parent: &'a ObservedEntry,
        name: &'a EntryName,
    ) -> StorageFuture<'a, ObservedEntry> {
        Box::pin(async move {
            let path = parent.entry().path().join(name).map_err(|_| {
                failure(self.operation(), Failure::Rejected(Rejection::PathTooLong))
            })?;
            self.bind(path, EntryType::file(None))
        })
    }

    fn read_link<'a>(&'a self, _: &'a ObservedEntry) -> StorageFuture<'a, LinkTarget> {
        Box::pin(async move {
            Err(failure(
                self.operation(),
                Failure::Rejected(Rejection::NotSymlink),
            ))
        })
    }

    fn verify(&self) -> StorageFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn execute_attribute<'a>(
        &'a self,
        _: Option<(&'a ObservedEntry, &'a EntryName)>,
        _: &'a ObservedEntry,
        request: AttributeRequest,
    ) -> StorageFuture<'a, AttributeResult> {
        Box::pin(async move {
            if !matches!(request.action(), AttributeAction::GetPermissions) {
                return Err(failure(
                    request.operation(),
                    Failure::Rejected(Rejection::Unsupported),
                ));
            }
            Permissions::new(0o444)
                .map(AttributeResult::Permissions)
                .map_err(|error| {
                    StorageError::without_effects(
                        Operation::GetPermissions,
                        Failure::Contract(ContractViolation::InvalidAttributes),
                        Some(Box::new(error)),
                    )
                })
        })
    }
}

struct AtomicMutationStorage;

impl Storage for AtomicMutationStorage {
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }

    fn mutation_binding(&self) -> MutationBinding {
        MutationBinding::FinalSlotAtomic
    }
}

#[test]
fn opened_storage_must_match_the_declared_mutation_binding() -> TestResult {
    let parent_pinned = Capabilities::default().with_remove(true);
    let atomic = parent_pinned.with_mutation_binding(MutationBinding::FinalSlotAtomic);
    let default_storage = TestStorage {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    validate_storage_extensions(parent_pinned, &default_storage)?;
    validate_storage_extensions(atomic, &AtomicMutationStorage)?;
    for (declared, storage) in [
        (atomic, &default_storage as &dyn Storage),
        (parent_pinned, &AtomicMutationStorage as &dyn Storage),
    ] {
        let error = validate_storage_extensions(declared, storage)
            .err()
            .ok_or("mismatched mutation binding accepted")?;
        assert_eq!(error.failure(), LifecycleFailure::InvalidDeclaration);
    }
    assert_eq!(default_storage.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn attribute_interface_and_access_levels_must_match_the_driver_declaration() -> TestResult {
    let declared = Capabilities::default().with_permissions(AttributeSupport::Read);
    let supported = PermissionStorage(ReadOnlyPermissions);
    validate_storage_extensions(declared, &supported)?;
    assert!(
        validate_storage_extensions(
            declared.with_permissions(AttributeSupport::ReadWrite),
            &supported,
        )
        .is_err()
    );
    assert!(
        validate_storage_extensions(declared.with_xattrs(AttributeSupport::Read), &supported,)
            .is_err()
    );
    let absent = TestStorage {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let missing = validate_storage_extensions(declared, &absent)
        .err()
        .ok_or("missing declared interface")?;
    assert_eq!(
        missing.failure(),
        ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration
    );
    let unexpected = validate_storage_extensions(Capabilities::default(), &supported)
        .err()
        .ok_or("unexpected interface")?;
    assert_eq!(
        unexpected.failure(),
        ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration
    );
    let xattrs = Capabilities::default().with_xattrs(AttributeSupport::Read);
    assert!(
        matches!(validate_storage_extensions(xattrs, &absent), Err(error) if error.failure() == ah_vfs::application::driver_error::LifecycleFailure::InvalidDeclaration)
    );
    assert_eq!(absent.calls.load(Ordering::SeqCst), 0);
    Ok(())
}
#[tokio::test]
async fn read_only_extension_and_missing_base_operations_reject_mutation() -> TestResult {
    let supported = Arc::new(PermissionStorage(ReadOnlyPermissions));
    let caps = Capabilities::default().with_permissions(AttributeSupport::Read);
    let service = entry_fixture(caps, None, supported.clone()).await?;
    assert_eq!(
        service
            .attribute("/file", AttributeAction::GetPermissions)
            .await?,
        AttributeResult::Permissions(Permissions::new(0o444)?)
    );
    let unavailable = entry_fixture(caps, Some(LifecycleFailure::Unavailable), supported).await?;
    assert!(
        matches!(unavailable.attribute("/file", AttributeAction::SetPermissions(Permissions::new(0o600)?)).await,
        Err(error) if error.storage_error().is_some_and(|error| error.failure() == Failure::Rejected(Rejection::Unsupported)))
    );
    let storage = TestStorage {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    assert!(
        matches!(storage.create_dir(ah_vfs_storage::CreateDirectoryRequest::new(StoragePath::parse("dir")?.to_rooted_expr(), ah_vfs_storage::CreateDirectoryMode::Single), Default::default()).await, Err(error) if error.failure() == Failure::Rejected(Rejection::Unsupported))
    );
    Ok(())
}
