use ah_vfs::application::mount_runtime::MountRuntime;
mod common;

use ah_vfs::{
    application::{
        driver_catalog::DriverCatalog,
        driver_info::DriverInfo,
        dto::{
            ReadFileRequest, StorageEntry, StorageReadFileRequest, StorageWriteFileMode,
            StorageWriteFileRequest, WriteFileRequest,
        },
        error::EntryError,
        port::{
            DriverFuture, Storage, StorageFileReader, StorageFileWriter, StorageFuture,
            StoragePermissions,
        },
        service::{EntryService, MountService},
    },
    domain::{
        AttributeSupport, ChildMountPolicy, DirModel, DriverError, DriverInstanceRef, DriverKey,
        EntryPermissions, FileReadSupport, FileWriteSupport, InstanceId, StorageCapabilities,
        StorageError, StorageErrorKind, StoragePath, VirtualPath, XattrName,
    },
    infrastructure::mount_repository::SqliteMountRepository,
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

fn failure(kind: StorageErrorKind) -> StorageError {
    StorageError::with_source(kind, io::Error::other("backend-private-token"))
}
struct TestReader;
impl StorageFileReader for TestReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async { Err(failure(StorageErrorKind::StaleHandle)) })
    }
}
struct TestWriter {
    closed: bool,
}
impl StorageFileWriter for TestWriter {
    fn write_chunk<'a>(&'a mut self, _: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async {
            if self.closed {
                Err(StorageError::new(StorageErrorKind::SessionClosed))
            } else {
                Ok(())
            }
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async {
            if self.closed {
                return Err(StorageError::new(StorageErrorKind::SessionClosed));
            }
            self.closed = true;
            Err(failure(StorageErrorKind::OutcomeUnknown))
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async {
            if self.closed {
                return Err(StorageError::new(StorageErrorKind::SessionClosed));
            }
            self.closed = true;
            Ok(())
        })
    }
}
struct TestStorage {
    capabilities: StorageCapabilities,
    calls: Arc<AtomicUsize>,
}
impl Storage for TestStorage {
    fn capabilities(&self) -> StorageCapabilities {
        self.capabilities
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::MIN
    }
    fn read_file(
        &self,
        _: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Box::new(TestReader) as Box<dyn StorageFileReader>) })
    }
    fn write_file(
        &self,
        _: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Box::new(TestWriter { closed: false }) as Box<dyn StorageFileWriter>) })
    }
}
struct TestDriver {
    info: DriverInfo,
    storage: Arc<dyn Storage>,
    open_failure: Option<StorageErrorKind>,
}
impl common::Backend for TestDriver {
    fn registry(self: Arc<Self>) -> Arc<dyn ah_vfs::application::port::InstanceRegistry> {
        common::static_registry(&["root"])
    }

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
                return Err(DriverError::Storage(failure(kind)));
            }
            Ok(self.storage.clone())
        })
    }
}
async fn fixture(
    actual: StorageCapabilities,
    declared: StorageCapabilities,
    open_failure: Option<StorageErrorKind>,
) -> Result<(EntryService, Arc<AtomicUsize>), Box<dyn Error>> {
    let key = DriverKey::parse("test")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let storage = Arc::new(TestStorage {
        capabilities: actual,
        calls: calls.clone(),
    });
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
    let runtime = Arc::new(MountRuntime::new(repository, drivers));
    MountService::new(runtime.clone())
        .initialize_root(DriverInstanceRef::new(key, InstanceId::parse("root")?))
        .await?;
    Ok((EntryService::new(runtime), calls))
}
fn request(atomic: bool) -> Result<WriteFileRequest, Box<dyn Error>> {
    Ok(WriteFileRequest {
        path: VirtualPath::parse("/file")?,
        mode: StorageWriteFileMode::CreateNew,
        require_atomic_publish: atomic,
    })
}

#[test]
fn normalized_errors_preserve_sources_without_exposing_backend_messages() -> TestResult {
    for kind in [
        StorageErrorKind::NotFound,
        StorageErrorKind::StaleHandle,
        StorageErrorKind::RateLimited,
        StorageErrorKind::Conflict,
        StorageErrorKind::TimedOut,
        StorageErrorKind::OutcomeUnknown,
    ] {
        let error = failure(kind);
        assert_eq!(error.kind(), kind);
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
                .kind(),
            kind
        );
        assert!(!entry.to_string().contains("backend-private-token"));
    }
    Ok(())
}

#[test]
fn capability_upper_bounds_preserve_semantics_and_read_only_attributes() {
    let upper = StorageCapabilities {
        read_file: FileReadSupport::Range,
        permissions: AttributeSupport::ReadWrite,
        dir_model: DirModel::Prefix,
        ..Default::default()
    };
    let actual = StorageCapabilities {
        read_file: FileReadSupport::Sequential,
        permissions: AttributeSupport::Read,
        dir_model: DirModel::Prefix,
        ..Default::default()
    };
    assert!(upper.supports(actual));
    assert!(!actual.supports(upper));
    assert!(!upper.supports(StorageCapabilities {
        dir_model: DirModel::Hierarchical,
        ..actual
    }));
    assert!(!StorageCapabilities::default().supports(actual));
    assert!(!actual.permissions.can_write());
}

#[test]
fn attribute_values_reject_invalid_input_without_normalizing_identity() -> TestResult {
    assert_eq!(EntryPermissions::new(0o7640)?.bits(), 0o7640);
    assert!(EntryPermissions::new(0o100644).is_err());
    for name in ["", "bad\0name", &"a".repeat(256)] {
        assert!(XattrName::parse(name).is_err());
    }
    assert_eq!(XattrName::parse("user.文档")?.as_str(), "user.文档");
    Ok(())
}

#[tokio::test]
async fn unsupported_requests_are_rejected_before_backend_side_effects() -> TestResult {
    let caps = StorageCapabilities {
        read_file: FileReadSupport::Sequential,
        write_file: FileWriteSupport {
            create_new: true,
            replace: false,
            atomic_publish: false,
        },
        ..Default::default()
    };
    let (service, calls) = fixture(caps, caps, None).await?;
    assert!(
        matches!(service.write_file(request(true)?).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::UnsupportedOperation))
    );
    assert!(
        matches!(service.read_file(ReadFileRequest { path: VirtualPath::parse("/file")?, offset: 1, length: None }).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::UnsupportedOperation))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut writer = service.write_file(request(false)?).await?;
    writer.abort().await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn opened_capabilities_must_match_declarations_and_extension_handles() -> TestResult {
    let caps = StorageCapabilities {
        read_file: FileReadSupport::Range,
        ..Default::default()
    };
    let (service, calls) = fixture(caps, StorageCapabilities::default(), None).await?;
    assert!(matches!(
        service.capabilities(&VirtualPath::root()).await,
        Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::InvalidData)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let caps = StorageCapabilities {
        permissions: AttributeSupport::Read,
        ..Default::default()
    };
    let (service, _) = fixture(caps, caps, None).await?;
    assert!(matches!(
        service.capabilities(&VirtualPath::root()).await,
        Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::InvalidData)
    ));
    Ok(())
}

#[tokio::test]
async fn opening_streaming_and_committing_keep_domain_error_categories() -> TestResult {
    let caps = StorageCapabilities {
        read_file: FileReadSupport::Range,
        write_file: FileWriteSupport {
            create_new: true,
            replace: false,
            atomic_publish: false,
        },
        ..Default::default()
    };
    let (service, _) = fixture(caps, caps, Some(StorageErrorKind::Unavailable)).await?;
    assert!(
        matches!(service.capabilities(&VirtualPath::root()).await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::Unavailable))
    );
    let (service, _) = fixture(caps, caps, None).await?;
    let mut reader = service
        .read_file(ReadFileRequest {
            path: VirtualPath::parse("/file")?,
            offset: 0,
            length: None,
        })
        .await?;
    assert!(
        matches!(reader.next_chunk().await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::StaleHandle))
    );
    let mut writer = service.write_file(request(false)?).await?;
    assert!(
        matches!(writer.commit().await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::OutcomeUnknown))
    );
    assert!(
        matches!(writer.commit().await, Err(error) if error.storage_error().is_some_and(|e| e.kind() == StorageErrorKind::SessionClosed))
    );
    Ok(())
}

struct ReadOnlyPermissions;
impl StoragePermissions for ReadOnlyPermissions {
    fn get_permissions(&self, _: StoragePath) -> StorageFuture<'_, EntryPermissions> {
        Box::pin(async { EntryPermissions::new(0o444) })
    }
}
#[tokio::test]
async fn read_only_extension_and_missing_base_operations_reject_mutation() -> TestResult {
    let extension = ReadOnlyPermissions;
    assert!(
        matches!(extension.set_permissions(StoragePath::root(), EntryPermissions::new(0o600)?).await, Err(error) if error.kind() == StorageErrorKind::UnsupportedOperation)
    );
    let storage = TestStorage {
        capabilities: StorageCapabilities::default(),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    assert!(
        matches!(storage.create_dir(ah_vfs::application::dto::StorageCreateDirRequest { path: StoragePath::parse("dir")?, recursive: false }).await, Err(error) if error.kind() == StorageErrorKind::UnsupportedOperation)
    );
    Ok(())
}

#[tokio::test]
async fn inconsistent_driver_capabilities_cannot_be_persisted_as_a_mount() -> TestResult {
    use ah_vfs::application::{
        dto::{MountFilter, MountRequest},
        port::MountRepository,
    };
    let actual = StorageCapabilities {
        read_file: FileReadSupport::Range,
        ..Default::default()
    };
    let key = DriverKey::parse("invalid-capabilities")?;
    let mut drivers = DriverCatalog::new();
    common::register(
        &mut drivers,
        common::managed(Arc::new(TestDriver {
            info: DriverInfo::builder()
                .key(key.clone())
                .name("Test".into())
                .version(semver::Version::new(0, 1, 0))
                .child_mount_policy(ChildMountPolicy::Allow)
                .capabilities(StorageCapabilities::default())
                .build(),
            storage: Arc::new(TestStorage {
                capabilities: actual,
                calls: Arc::new(AtomicUsize::new(0)),
            }),
            open_failure: None,
        })),
    )?;
    let repository = Arc::new(SqliteMountRepository::in_memory().await?);
    let mounts = MountService::new(Arc::new(MountRuntime::new(
        repository.clone(),
        Arc::new(drivers),
    )));
    let error = mounts
        .mount(MountRequest {
            virtual_path: VirtualPath::root(),
            driver_instance: DriverInstanceRef::new(key, InstanceId::parse("root")?),
        })
        .await
        .err()
        .ok_or("expected invalid adapter")?;
    assert_eq!(
        error
            .storage_error()
            .ok_or("missing normalized failure")?
            .kind(),
        StorageErrorKind::InvalidData
    );
    assert!(repository.list(MountFilter::default()).await?.is_empty());
    Ok(())
}
