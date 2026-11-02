//! Typed guest ports and a single protobuf boundary for all WASM plugins.
use std::{collections::BTreeMap, marker::PhantomData};
use vfs_plugin_protocol::{self as p, Operation as Op, Value};

pub mod api {
    use super::*;
    pub type Failure = p::Failure;
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
        fn read(&self, path: String, offset: u64, length: Option<u64>) -> Result<Reader>;
        fn write(&self, path: String, replace: bool, atomic: bool) -> Result<Writer>;
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
        let mut mutates = false;
        let mut allocates = false;
        let result = p::decode_request(bytes)
            .map_err(|error| {
                p::Failure::new(match error {
                    p::CodecError::TooLarge => p::FailureCode::TooLarge,
                    p::CodecError::InvalidData => p::FailureCode::InvalidRequest,
                })
            })
            .and_then(|(generation, operation, data)| {
                mutates = operation.mutates();
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
        p::encode_reply(result).unwrap_or_else(|error| {
            if let Some(handle) = allocated {
                self.resources.remove(&handle);
            }
            // Failure to encode a mutation's result cannot establish rollback.
            let code = if mutates {
                p::wire::FailureCode::OutcomeUnknown
            } else {
                match error {
                    p::CodecError::TooLarge => p::wire::FailureCode::TooLarge,
                    p::CodecError::InvalidData => p::wire::FailureCode::InvalidData,
                }
            };
            p::encode_status(code)
        })
    }
    fn capacity(&self) -> api::Result<()> {
        if self.resources.len() >= p::MAX_RESOURCES || self.next == u64::MAX {
            Err(p::Failure::new(p::FailureCode::CapacityExceeded))
        } else {
            Ok(())
        }
    }
    fn insert(&mut self, resource: Resource) -> u64 {
        self.next += 1;
        self.resources.insert(self.next, resource);
        self.next
    }
    fn storage(&self, handle: u64) -> api::Result<&dyn api::GuestStorage> {
        match self.resources.get(&handle) {
            Some(Resource::Storage(storage)) => Ok(storage.0.as_ref()),
            Some(_) => Err(p::Failure::new(p::FailureCode::InvalidRequest)),
            None => Err(p::Failure::new(p::FailureCode::SessionClosed)),
        }
    }
    fn execute(&mut self, generation: &str, operation: Op, data: Vec<u8>) -> api::Result<p::Reply> {
        if generation.is_empty() {
            return Err(p::Failure::new(p::FailureCode::InvalidRequest));
        }
        if let Some(expected) = &self.generation {
            if expected != generation {
                return Err(p::Failure::new(p::FailureCode::StaleHandle));
            }
        } else if matches!(operation, Op::Describe(p::Empty {})) {
            self.generation = Some(generation.into());
        } else {
            return Err(p::Failure::new(p::FailureCode::Unavailable));
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
                        return Err(p::Failure::new(p::FailureCode::Conflict));
                    }
                } else {
                    D::initialize(settings.clone())?;
                    self.settings = Some(settings);
                }
                Value::Unit(p::Empty {})
            }
            _ if self.settings.is_none() => {
                return Err(p::Failure::new(p::FailureCode::Unavailable));
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
                self.capacity()?;
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
                operation: Some(request),
            }) => return self.storage(handle)?.call(request, data),
            Op::Read(p::Read {
                handle,
                path,
                offset,
                length,
            }) => {
                self.capacity()?;
                let reader = self.storage(handle)?.read(path, offset, length)?;
                Value::Reader(p::Handle {
                    handle: self.insert(Resource::Reader(reader)),
                })
            }
            Op::Write(p::Write {
                handle,
                path,
                replace,
                atomic,
            }) => {
                self.capacity()?;
                let writer = self.storage(handle)?.write(path, replace, atomic)?;
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
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest));
                }
                let Some(Resource::Reader(reader)) = self.resources.remove(&handle) else {
                    return Err(p::Failure::new(p::FailureCode::SessionClosed));
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
                    Some(_) => return Err(p::Failure::new(p::FailureCode::InvalidData)),
                }
            }
            Op::WriterChunk(p::Handle { handle }) => {
                let Some(resource) = self.resources.get(&handle) else {
                    return Err(p::Failure::new(p::FailureCode::SessionClosed));
                };
                let Resource::Writer(writer) = resource else {
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest));
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
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest));
                }
                let Some(Resource::Writer(writer)) = self.resources.remove(&handle) else {
                    return Err(p::Failure::new(p::FailureCode::SessionClosed));
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
                    return Err(p::Failure::new(p::FailureCode::InvalidRequest));
                }
                self.resources.remove(&handle);
                Value::Unit(p::Empty {})
            }
            _ => return Err(p::Failure::new(p::FailureCode::InvalidRequest)),
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
