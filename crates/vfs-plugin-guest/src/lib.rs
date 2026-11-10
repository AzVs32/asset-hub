//! Typed guest ports and a single protobuf boundary for all WASM plugins.
use std::{collections::BTreeMap, marker::PhantomData};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub mod wasi_http;

pub mod api {
    use super::*;
    pub use crate::error::Failure;
    pub use crate::error_context::ErrorContext;
    pub use crate::lookup::{direct_lookup, direct_stat, direct_write_destination};
    pub type Result<T> = std::result::Result<T, Failure>;
    pub trait Guest {
        fn describe() -> p::Description;
        /// Called once per control/Storage Store; independent Stores may run concurrently.
        /// Instance records and decrypted opening parameters are supplied by the host.
        fn initialize(settings: p::Object) -> Result<()>;
        fn prepare(config: p::Object) -> Result<p::Prepared>;
        fn migrate(from_version: u32, config: p::Object) -> Result<p::Object>;
        fn test_connection(config: p::Object) -> Result<()>;

        fn open(id: String, config: p::Object) -> Result<Storage>;
    }
    pub trait GuestStorage {
        fn info(&self) -> p::StorageInfo;
        fn call(&self, request: p::StorageOp, data: Vec<u8>) -> Result<p::Reply>;
        fn remove(
            &self,
            _request: &ah_vfs_storage::RemoveRequest,
            _base: &ah_vfs_storage::StoragePath,
            _links: ah_vfs_storage::IntermediateSymlinks,
        ) -> Result<()> {
            Err(
                ErrorContext::storage(ah_vfs_storage::Operation::Remove).failure(
                    ah_vfs_storage::Failure::Rejected(ah_vfs_storage::Rejection::Unsupported),
                ),
            )
        }
        fn rename(
            &self,
            _request: &ah_vfs_storage::RenameRequest,
            _base: &ah_vfs_storage::StoragePath,
            _links: ah_vfs_storage::IntermediateSymlinks,
        ) -> Result<ah_vfs_storage::Entry> {
            Err(
                ErrorContext::storage(ah_vfs_storage::Operation::Rename).failure(
                    ah_vfs_storage::Failure::Rejected(ah_vfs_storage::Rejection::Unsupported),
                ),
            )
        }
        fn create_dir(
            &self,
            _request: &ah_vfs_storage::CreateDirectoryRequest,
            _base: &ah_vfs_storage::StoragePath,
            _links: ah_vfs_storage::IntermediateSymlinks,
        ) -> Result<ah_vfs_storage::Entry> {
            Err(
                ErrorContext::storage(ah_vfs_storage::Operation::CreateDirectory).failure(
                    ah_vfs_storage::Failure::Rejected(ah_vfs_storage::Rejection::Unsupported),
                ),
            )
        }
        fn select_directory(
            &self,
            _request: &ah_vfs_storage::ListRequest,
            _base: &ah_vfs_storage::StoragePath,
            _links: ah_vfs_storage::IntermediateSymlinks,
        ) -> Result<ah_vfs_storage::DirectorySelection> {
            Err(
                ErrorContext::storage(ah_vfs_storage::Operation::List).failure(
                    ah_vfs_storage::Failure::Rejected(ah_vfs_storage::Rejection::Unsupported),
                ),
            )
        }
        fn list_page(
            &self,
            _bound: &ah_vfs_storage::BoundListRequest<'_>,
        ) -> Result<ah_vfs_storage::DirectoryPage> {
            Err(
                ErrorContext::storage(ah_vfs_storage::Operation::List).failure(
                    ah_vfs_storage::Failure::Rejected(ah_vfs_storage::Rejection::Unsupported),
                ),
            )
        }
        fn read(
            &self,
            request: ah_vfs_storage::ReadRequest,
            working_directory: ah_vfs_storage::StoragePath,
            intermediate_symlinks: ah_vfs_storage::IntermediateSymlinks,
        ) -> Result<Reader>;
        fn write(
            &self,
            request: ah_vfs_storage::WriteRequest,
            working_directory: ah_vfs_storage::StoragePath,
            intermediate_symlinks: ah_vfs_storage::IntermediateSymlinks,
        ) -> Result<Writer>;
    }
    pub trait GuestReader {
        fn next(&self) -> Result<Option<Vec<u8>>>;
    }
    pub trait GuestWriter {
        fn write(&self, data: Vec<u8>) -> Result<()>;
        fn commit(&self) -> Result<p::Entry>;
        fn abort(&self) -> Result<()>;
    }
    pub struct Storage(pub(crate) Box<dyn GuestStorage>);
    impl Storage {
        pub fn new(value: impl GuestStorage + 'static) -> Self {
            Self(Box::new(value))
        }
    }
    pub struct Reader(pub(crate) Box<dyn GuestReader>);
    impl Reader {
        pub fn new(value: impl GuestReader + 'static) -> Self {
            Self(Box::new(value))
        }
    }
    pub struct Writer(pub(crate) Box<dyn GuestWriter>);
    impl Writer {
        pub fn new(value: impl GuestWriter + 'static) -> Self {
            Self(Box::new(value))
        }
    }
}
enum Resource {
    Storage(api::Storage),
    Reader(api::Reader),
    Writer(api::Writer),
}
impl Resource {
    fn kind(&self) -> p::ResourceKind {
        match self {
            Self::Storage(_) => p::ResourceKind::Storage,
            Self::Reader(_) => p::ResourceKind::Reader,
            Self::Writer(_) => p::ResourceKind::Writer,
        }
    }
}
pub struct Dispatcher<D> {
    generation: Option<String>,
    settings: Option<p::Object>,
    resources: BTreeMap<u64, Resource>,
    next: u64,
    driver: PhantomData<D>,
}
impl<D: api::Guest> Default for Dispatcher<D> {
    fn default() -> Self {
        Self::new()
    }
}
impl<D: api::Guest> Dispatcher<D> {
    pub fn new() -> Self {
        Self {
            generation: None,
            settings: None,
            resources: BTreeMap::new(),
            next: 0,
            driver: PhantomData,
        }
    }
    pub fn call(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut context = api::ErrorContext::lifecycle();
        let mut allocates = false;
        let result = p::decode_request(bytes)
            .map_err(|error| {
                p::Failure::new(match error {
                    p::CodecError::TooLarge => p::FailureCode::TooLarge,
                    p::CodecError::InvalidData | p::CodecError::Decode(_) => {
                        p::FailureCode::InvalidRequest
                    }
                })
            })
            .map_err(api::Failure::from)
            .and_then(|(generation, operation, data)| {
                context = api::ErrorContext::for_request(&operation);
                if operation.may_publish() {
                    context.dispatched();
                }
                allocates = matches!(operation, Op::Open(_) | Op::Read(_) | Op::Write(_));
                self.execute(&generation, operation, data)
            });
        let allocated = if allocates {
            match &result {
                Ok(p::Reply {
                    result:
                        Some(
                            Value::Storage(p::Storage { handle, .. })
                            | Value::Reader(p::Handle { handle })
                            | Value::Writer(p::Handle { handle }),
                        ),
                    ..
                }) => Some(*handle),
                _ => None,
            }
        } else {
            None
        };
        p::encode_reply(result.map_err(p::Failure::from)).unwrap_or_else(|_| {
            if let Some(handle) = allocated {
                self.resources.remove(&handle);
            }
            let error = context.failure(ah_vfs_storage::Failure::Contract(
                ah_vfs_storage::ContractViolation::InvalidResponse,
            ));
            p::encode_reply(Err(error.into()))
                .unwrap_or_else(|_| p::encode_status(p::FailureCode::ProtocolViolation))
        })
    }
    fn capacity(&self, context: &api::ErrorContext) -> api::Result<()> {
        if self.resources.len() >= p::MAX_RESOURCES || self.next == u64::MAX {
            Err(context.failure(ah_vfs_storage::Failure::Backend(
                ah_vfs_storage::BackendFailure::CapacityExceeded,
            )))
        } else {
            Ok(())
        }
    }
    fn insert(&mut self, resource: Resource) -> u64 {
        self.next += 1;
        self.resources.insert(self.next, resource);
        self.next
    }
    fn storage(
        &self,
        context: &api::ErrorContext,
        handle: u64,
    ) -> api::Result<&dyn api::GuestStorage> {
        match self.resources.get(&handle) {
            Some(Resource::Storage(storage)) => Ok(storage.0.as_ref()),
            Some(_) => Err(p::Failure::new(p::FailureCode::InvalidRequest).into()),
            None => Err(context.failure(ah_vfs_storage::Failure::Backend(
                ah_vfs_storage::BackendFailure::Closed,
            ))),
        }
    }
    fn execute(&mut self, generation: &str, operation: Op, data: Vec<u8>) -> api::Result<p::Reply> {
        let context = api::ErrorContext::for_request(&operation);
        if generation.is_empty() {
            return Err(p::Failure::new(p::FailureCode::InvalidRequest).into());
        }
        if let Some(expected) = &self.generation {
            if expected != generation {
                return Err(context.failure(ah_vfs_storage::Failure::Backend(
                    ah_vfs_storage::BackendFailure::Stale,
                )));
            }
        } else if matches!(operation, Op::Describe(p::Empty {})) {
            self.generation = Some(generation.into());
        } else {
            return Err(context.failure(ah_vfs_storage::Failure::Backend(
                ah_vfs_storage::BackendFailure::Unavailable,
            )));
        }
        let value = match operation {
            Op::Describe(p::Empty {}) => {
                let mut description = D::describe();
                description.generation = generation.into();
                Value::Description(description.into())
            }
            Op::Initialize(p::Initialize {
                settings: Some(settings),
            }) => {
                if let Some(previous) = &self.settings {
                    if previous != &settings {
                        return Err(context.failure(ah_vfs_storage::Failure::Rejected(
                            ah_vfs_storage::Rejection::Conflict,
                        )));
                    }
                } else {
                    D::initialize(settings.clone())?;
                    self.settings = Some(settings);
                }
                Value::Unit(p::Empty {})
            }
            _ if self.settings.is_none() => {
                return Err(context.failure(ah_vfs_storage::Failure::Backend(
                    ah_vfs_storage::BackendFailure::Unavailable,
                )));
            }
            Op::Prepare(p::Config {
                config: Some(config),
            }) => {
                let prepared = D::prepare(config)?;
                Value::Prepared(prepared)
            }
            Op::Migrate(p::Migrate {
                from_version,
                config: Some(config),
            }) => Value::Config(p::Config {
                config: Some(D::migrate(from_version, config)?),
            }),
            Op::TestConnection(p::Config {
                config: Some(config),
            }) => {
                D::test_connection(config)?;
                Value::Unit(p::Empty {})
            }
            Op::Open(p::Open {
                id,
                config: Some(config),
            }) => {
                self.capacity(&context)?;
                let storage = D::open(id, config)?;
                let info = storage.0.info();
                let handle = self.insert(Resource::Storage(storage));
                Value::Storage(p::Storage {
                    handle,
                    info: Some(info),
                })
            }
            Op::Storage(p::StorageCall {
                handle,
                operation: Some(p::StorageOp::Remove(wire)),
            }) => {
                use ah_vfs_storage::{Failure, Rejection};
                let (request, base, links) = p::decode_remove(wire)
                    .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
                let capabilities = p::decode_capabilities(&D::describe().capabilities)
                    .map_err(|reason| context.failure(Failure::Contract(reason)))?;
                if !capabilities.remove() {
                    return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
                }
                self.storage(&context, handle)?
                    .remove(&request, &base, links)?;
                Value::Unit(p::Empty {})
            }
            Op::Storage(p::StorageCall {
                handle,
                operation: Some(p::StorageOp::Rename(wire)),
            }) => {
                use ah_vfs_storage::{ContractViolation, Failure, Operation, Rejection};
                let (request, base, links) = p::decode_rename(wire)
                    .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
                let capabilities = p::decode_capabilities(&D::describe().capabilities)
                    .map_err(|reason| context.failure(Failure::Contract(reason)))?;
                if !capabilities.rename().supports(request.mode()) {
                    return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
                }
                let entry = self
                    .storage(&context, handle)?
                    .rename(&request, &base, links)?;
                Value::Entry(p::encode_entry(&entry).map_err(|_| {
                    ah_vfs_storage::StorageError::after_dispatch(
                        Operation::Rename,
                        Failure::Contract(ContractViolation::InvalidEntry),
                        None,
                    )
                })?)
            }
            Op::Storage(p::StorageCall {
                handle,
                operation: Some(p::StorageOp::CreateDirectory(wire)),
            }) => {
                use ah_vfs_storage::{ContractViolation, Failure, Operation, Rejection};
                let (request, base, links) = p::decode_create_directory(wire)
                    .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
                let capabilities = p::decode_capabilities(&D::describe().capabilities)
                    .map_err(|reason| context.failure(Failure::Contract(reason)))?;
                if !capabilities.directories().can_create() {
                    return Err(context.failure(Failure::Rejected(Rejection::Unsupported)));
                }
                let entry = self
                    .storage(&context, handle)?
                    .create_dir(&request, &base, links)?;
                Value::Entry(p::encode_entry(&entry).map_err(|_| {
                    ah_vfs_storage::StorageError::after_dispatch(
                        Operation::CreateDirectory,
                        Failure::Contract(ContractViolation::InvalidEntry),
                        None,
                    )
                })?)
            }
            Op::Storage(p::StorageCall {
                handle,
                operation: Some(p::StorageOp::SelectDirectory(wire)),
            }) => {
                let (request, base, links) = p::decode_select_directory(wire)
                    .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
                let selection = self
                    .storage(&context, handle)?
                    .select_directory(&request, &base, links)?;
                request
                    .bind_selection(selection.clone())
                    .map_err(|reason| context.failure(ah_vfs_storage::Failure::Backend(reason)))?;
                Value::SelectedDirectory(p::encode_selection(&selection))
            }
            Op::Storage(p::StorageCall {
                handle,
                operation: Some(p::StorageOp::List(wire)),
            }) => {
                let (request, selection) = p::decode_list(wire)
                    .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
                let bound = request
                    .bind_selection(selection)
                    .map_err(|reason| context.failure(ah_vfs_storage::Failure::Backend(reason)))?;
                let page = self.storage(&context, handle)?.list_page(&bound)?;
                if page.selection() != bound.selection()
                    || page.entries().len() > bound.size().get()
                {
                    return Err(context.failure(ah_vfs_storage::Failure::Contract(
                        ah_vfs_storage::ContractViolation::InvalidPage,
                    )));
                }
                return p::encode_page(page)
                    .map_err(|reason| context.failure(ah_vfs_storage::Failure::Contract(reason)));
            }
            Op::Storage(p::StorageCall {
                handle,
                operation: Some(request),
            }) => return self.storage(&context, handle)?.call(request, data),
            Op::Read(read) => {
                let handle = read.handle;
                let (request, base, links) = p::decode_read(read)
                    .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
                p::decode_capabilities(&D::describe().capabilities)
                    .map_err(|cause| context.failure(ah_vfs_storage::Failure::Contract(cause)))?
                    .require_read(&request)
                    .map_err(|reason| context.failure(ah_vfs_storage::Failure::Rejected(reason)))?;
                self.capacity(&context)?;
                let reader = self.storage(&context, handle)?.read(request, base, links)?;
                Value::Reader(p::Handle {
                    handle: self.insert(Resource::Reader(reader)),
                })
            }
            Op::Write(write) => {
                let handle = write.handle;
                let (request, base, links) = p::decode_write(write)
                    .map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
                p::decode_capabilities(&D::describe().capabilities)
                    .map_err(|cause| context.failure(ah_vfs_storage::Failure::Contract(cause)))?
                    .require_write(&request)
                    .map_err(|reason| context.failure(ah_vfs_storage::Failure::Rejected(reason)))?;
                self.capacity(&context)?;
                let writer = self
                    .storage(&context, handle)?
                    .write(request, base, links)?;
                Value::Writer(p::Handle {
                    handle: self.insert(Resource::Writer(writer)),
                })
            }
            Op::ReaderNext(p::Handle { handle }) => {
                if self
                    .resources
                    .get(&handle)
                    .is_some_and(|r| r.kind() != p::ResourceKind::Reader)
                {
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest).into());
                }
                let Some(Resource::Reader(reader)) = self.resources.remove(&handle) else {
                    return Err(context.failure(ah_vfs_storage::Failure::Backend(
                        ah_vfs_storage::BackendFailure::Closed,
                    )));
                };
                match reader.0.next()? {
                    None => Value::Chunk(p::Chunk { eof: true }),
                    Some(bytes) if !bytes.is_empty() && bytes.len() <= p::MAX_DATA_BYTES => {
                        self.resources.insert(handle, Resource::Reader(reader));
                        return Ok(p::Reply {
                            result: Some(Value::Chunk(p::Chunk { eof: false })),
                            data: bytes,
                        });
                    }
                    Some(_) => {
                        return Err(p::Failure::new(p::FailureCode::ProtocolViolation).into());
                    }
                }
            }
            Op::WriterChunk(p::Handle { handle }) => {
                let Some(resource) = self.resources.get(&handle) else {
                    return Err(context.failure(ah_vfs_storage::Failure::Backend(
                        ah_vfs_storage::BackendFailure::Closed,
                    )));
                };
                let Resource::Writer(writer) = resource else {
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest).into());
                };
                if let Err(error) = writer.0.write(data) {
                    self.resources.remove(&handle);
                    return Err(error);
                }
                Value::Unit(p::Empty {})
            }
            op @ (Op::Commit(p::Handle { handle }) | Op::Abort(p::Handle { handle })) => {
                if self
                    .resources
                    .get(&handle)
                    .is_some_and(|r| r.kind() != p::ResourceKind::Writer)
                {
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest).into());
                }
                let Some(Resource::Writer(writer)) = self.resources.remove(&handle) else {
                    return Err(context.failure(ah_vfs_storage::Failure::Backend(
                        ah_vfs_storage::BackendFailure::Closed,
                    )));
                };
                if matches!(op, Op::Commit(_)) {
                    Value::Entry(writer.0.commit()?)
                } else {
                    writer.0.abort()?;
                    Value::Unit(p::Empty {})
                }
            }
            Op::Release(p::ResourceRef { kind, handle }) => {
                if self
                    .resources
                    .get(&handle)
                    .is_some_and(|r| r.kind() as i32 != kind)
                {
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest).into());
                }
                self.resources.remove(&handle);
                Value::Unit(p::Empty {})
            }
            _ => return Err(p::Failure::new(p::FailureCode::InvalidRequest).into()),
        };
        Ok(p::Reply::new(value))
    }
}

/// Bind the small WIT transport and host-service imports in the guest crate.
/// Every exported call decodes Request and returns Reply from the shared schema.
#[macro_export]
macro_rules! export_plugin {
    ($driver:ty, $wit_path:literal) => {
        wit_bindgen::generate!({ path: $wit_path, world: "plugin" });
        #[cfg(target_arch = "wasm32")]
        struct VfsComponent;
        #[cfg(target_arch = "wasm32")]
        std::thread_local! {
            static VFS_DISPATCHER: std::cell::RefCell<$crate::Dispatcher<$driver>> = std::cell::RefCell::new($crate::Dispatcher::new());
        }
        #[cfg(target_arch = "wasm32")]
        impl exports::assethub::vfs::driver::Guest for VfsComponent {
            fn call(request: Vec<u8>) -> Vec<u8> {
                VFS_DISPATCHER.with(|dispatcher| dispatcher.borrow_mut().call(&request))
            }
        }
        #[cfg(target_arch = "wasm32")]
        export!(VfsComponent);
    };
}

mod error_context;
mod lookup;

mod error;
