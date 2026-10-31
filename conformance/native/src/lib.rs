//! Native ports for the shared conformance backend. The gRPC executable uses
//! this exact driver through the public native server SDK.
use ah_vfs::{
    application::{driver_info::DriverInfo, dto::*, port::*},
    domain::*,
};
use conformance_model as m;
use std::{num::NonZeroUsize, sync::Arc};
use vfs_plugin_codec as c;

fn error(error: vfs_plugin_protocol::Failure) -> StorageError {
    let error = c::storage_error(c::driver_error(error, None));
    if error.kind() == StorageErrorKind::Backend {
        StorageError::with_source(
            error.kind(),
            std::io::Error::other("conformance-private-diagnostic"),
        )
    } else {
        error
    }
}
async fn wait(config: &m::Config, phase: &str) -> Result<(), StorageError> {
    let origin = config
        .gate
        .as_ref()
        .ok_or_else(|| StorageError::new(StorageErrorKind::InvalidRequest))?;
    reqwest::Client::new()
        .get(format!("{origin}/{phase}"))
        .send()
        .await
        .map_err(|_| StorageError::new(StorageErrorKind::Unavailable))?
        .error_for_status()
        .map_err(|_| StorageError::new(StorageErrorKind::Backend))?;
    Ok(())
}

pub struct ConformanceDriver {
    info: DriverInfo,
    world: Arc<m::World>,
    provisioner: Arc<Provisioner>,
}
impl Default for ConformanceDriver {
    fn default() -> Self {
        Self::new()
    }
}
impl ConformanceDriver {
    pub fn new() -> Self {
        Self {
            info: c::driver_info(&m::description()).expect("fixture declaration"),
            world: m::World::new(),
            provisioner: Arc::new(Provisioner),
        }
    }
}
struct Provisioner;
impl InstanceProvisioner for Provisioner {
    fn prepare(&self, config: InstanceConfig) -> DriverFuture<'_, PreparedConfig> {
        Box::pin(async move {
            m::prepare(&config).map_err(|e| c::driver_error(e, None))?;
            Ok(PreparedConfig::new(config, Default::default()))
        })
    }
    fn migrate_config(
        &self,
        from: u32,
        config: InstanceConfig,
    ) -> DriverFuture<'_, InstanceConfig> {
        Box::pin(async move {
            if from != 1 {
                return Err(StorageError::new(StorageErrorKind::UnsupportedOperation).into());
            }
            m::prepare(&config).map_err(|e| c::driver_error(e, None))?;
            Ok(config)
        })
    }
    fn test_connection<'a>(&'a self, config: &'a InstanceConfig) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            m::prepare(config)
                .map(|_| ())
                .map_err(|e| c::driver_error(e, None))
        })
    }
}
impl Driver for ConformanceDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }
    fn instances(&self) -> InstanceManagement {
        InstanceManagement::HostManaged(self.provisioner.clone())
    }
    fn open(&self, context: OpenContext) -> DriverFuture<'_, Arc<dyn Storage>> {
        Box::pin(async move {
            let config = context
                .config()
                .ok_or_else(|| StorageError::new(StorageErrorKind::InvalidRequest))?;
            let config = m::prepare(config).map_err(|e| c::driver_error(e, None))?;
            if context.instance_id().as_str() == "wait-open" {
                wait(&config, "open").await?;
            }
            Ok(
                Arc::new(NativeStorage(m::Storage::new(self.world.clone(), config)))
                    as Arc<dyn Storage>,
            )
        })
    }
}
struct NativeStorage(m::Storage);
impl Storage for NativeStorage {
    fn capabilities(&self) -> StorageCapabilities {
        c::decode_capabilities(&self.0.info().capabilities).expect("fixture capabilities")
    }
    fn chunk_size(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.0.info().chunk_size as usize).unwrap_or(NonZeroUsize::MIN)
    }
    fn stat_entry(&self, r: StorageStatEntryRequest) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move { c::decode_entry(self.0.stat(r.path.as_str()).map_err(error)?) })
    }
    fn list_entries(&self, q: StorageDirQuery) -> StorageFuture<'_, StorageDirPage> {
        Box::pin(async move {
            q.validate()?;
            let (entries, next_after) = self
                .0
                .list(q.path.as_str(), q.after.as_deref(), q.limit.get() as u32)
                .map_err(error)?;
            Ok(StorageDirPage {
                entries: entries
                    .into_iter()
                    .map(c::decode_entry)
                    .collect::<Result<_, _>>()?,
                next_after,
            })
        })
    }
    fn remove_entry(&self, r: StorageRemoveEntryRequest) -> StorageFuture<'_, ()> {
        Box::pin(async move { self.0.remove(r.path.as_str()).map_err(error) })
    }
    fn read_file(
        &self,
        r: StorageReadFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileReader>> {
        Box::pin(async move {
            Ok(Box::new(NativeReader {
                inner: Some(
                    self.0
                        .read(r.path.as_str(), r.offset, r.length)
                        .map_err(error)?,
                ),
                config: self.0.config.clone(),
                block: r.path.as_str() == "wait-read",
                eof: false,
            }) as Box<dyn StorageFileReader>)
        })
    }
    fn write_file(
        &self,
        r: StorageWriteFileRequest,
    ) -> StorageFuture<'_, Box<dyn StorageFileWriter>> {
        Box::pin(async move {
            Ok(Box::new(NativeWriter {
                inner: Some(
                    self.0
                        .write(r.path.as_str(), r.mode == StorageWriteFileMode::Replace)
                        .map_err(error)?,
                ),
                config: self.0.config.clone(),
            }) as Box<dyn StorageFileWriter>)
        })
    }
}
struct NativeReader {
    inner: Option<m::Reader>,
    config: m::Config,
    block: bool,
    eof: bool,
}
impl StorageFileReader for NativeReader {
    fn next_chunk(&mut self) -> StorageFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            if self.eof {
                return Ok(None);
            }
            let mut reader = self
                .inner
                .take()
                .ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
            if self.block {
                wait(&self.config, "read").await?;
                self.block = false;
            }
            let result = reader.next_chunk().map_err(error)?;
            if result.is_some() {
                self.inner = Some(reader);
            } else {
                self.eof = true;
            }
            Ok(result)
        })
    }
}
struct NativeWriter {
    inner: Option<m::Writer>,
    config: m::Config,
}
impl StorageFileWriter for NativeWriter {
    fn write_chunk<'a>(&'a mut self, data: &'a [u8]) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let mut writer = self
                .inner
                .take()
                .ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
            if writer.path == "wait-write" {
                wait(&self.config, "write").await?;
            }
            writer.write(data).map_err(error)?;
            self.inner = Some(writer);
            Ok(())
        })
    }
    fn commit(&mut self) -> StorageFuture<'_, StorageEntry> {
        Box::pin(async move {
            let pending = self
                .inner
                .take()
                .ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?
                .take_commit()
                .map_err(error)?;
            if pending.path == "wait-commit" {
                wait(&self.config, "commit").await?;
            }
            let after_publish = pending.path == "wait-reply";
            let entry = pending.publish().map_err(error)?;
            if after_publish {
                wait(&self.config, "reply").await?;
            }
            c::decode_entry(entry)
        })
    }
    fn abort(&mut self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            let mut writer = self
                .inner
                .take()
                .ok_or_else(|| StorageError::new(StorageErrorKind::SessionClosed))?;
            writer.abort().map_err(error)?;
            if writer.path == "wait-abort" {
                wait(&self.config, "abort").await?;
            }
            Ok(())
        })
    }
}
