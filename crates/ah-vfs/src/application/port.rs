//! Interfaces implemented by infrastructure adapters.
//!
//! # Plugin boundary contract
//!
//! Native drivers implement these Rust ports directly. RPC and WASM adapters
//! implement the same ports on the host; Rust objects, references and futures
//! never cross the plugin boundary. RPC bindings use Protobuf messages and
//! streaming file transfers. Both transports use the shared protobuf schema;
//! WIT supplies the WASM call export and explicitly granted host services.
//! Bindings must preserve the following shared semantics.
//!
//! ## Loading and declarations
//!
//! The loader connects or instantiates the plugin, negotiates a protocol version
//! and fetches its description before constructing a Driver. The protocol's
//! major/minor version is independent of driver release and configuration schema
//! versions. A major mismatch or missing required feature rejects loading; minor
//! negotiation selects the lower mutually supported revision. Unknown optional
//! capabilities are unsupported, never inferred from a newer version number.
//! The description includes DriverInfo and supported
//! protocol features. Validate identities, schemas and driver capabilities before
//! registration. Validate transfer limits when opening each storage session.
//! Initialize applies plugin-wide DriverSettings;
//! only successful initialization publishes the registration. All synchronous
//! declarations are local immutable snapshots, including after reconnects.
//! A reconnect with changed declarations requires a new registration rather than
//! mutating existing proxies.
//! Bootstrap settings (endpoint, executable/component, authentication and host
//! permissions) belong to the loader, separate from DriverSettings and instance
//! configuration. Dynamic settings use the described schema rather than requiring
//! a host-linked Rust configuration declaration for each plugin.
//! Instance schemas use `assethub.config.annotations.v1`: the host locates
//! x-secret/x-immutable at the root and through properties/items/additionalProperties.
//! References and annotations inside unsupported constructs reject loading or
//! registration. Drivers retain authority over validation of configuration values.
//! WASM plugins import standard WASIp2 services. Filesystem, environment,
//! TCP and HTTP authority is granted per plugin; missing imports or ungranted
//! required capabilities reject loading. CPU budgets and wall deadlines bound execution.
//!
//! ## Operations and values
//!
//! The operation groups are describe/initialize, instance management, open,
//! storage operations, reader next/close, and writer write/commit/abort/close.
//! Drivers expose InstanceProvisioner operations; the host persists records and supplies
//! decrypted OpenContext configuration. Preparation and migration may await plugin
//! execution but never persist host records or open storage. The host revalidates
//! migrated configuration before a revision-checked write.
//!
//! Bindings use explicit fixed-width integers, not Rust discriminants or usize.
//! Byte offsets and lengths are u64; bounded counts and transfer sizes are u32
//! and must be validated before conversion. Timestamps use signed Unix seconds
//! plus a u32 nanosecond fraction below one billion. Paths and names preserve the
//! existing UTF-8 validation and ordering rules. Configurations and schemas are
//! JSON objects encoded without lossy numeric conversion (for example, UTF-8 JSON
//! bytes in Protobuf, rather than a floating-point-only dynamic object). File and
//! attribute bytes remain binary. Every decoded request and response is validated;
//! resource counts, page sizes, frame sizes and buffered bytes are bounded.
//!
//! ## Errors and cancellation
//!
//! Bindings explicitly map named error categories, never Rust enum ordinals,
//! Debug strings or backend error messages. DriverError distinguishes invalid
//! configuration (safe field identifier plus InstanceConfigErrorKind), missing
//! instance, lifecycle failures, and StorageError. Storage failures preserve the
//! operation, cause and mutation outcome independently; unknown remote tags are
//! rejected as contract violations. Diagnostics and credentials are not
//! transmitted in public errors. RPC status or a WASM trap alone does not establish
//! whether a mutation happened. Operations without side effects preserve transport
//! categories such as Unavailable, TimedOut and Cancelled. A dispatched mutation
//! with uncertain effects carries MutationOutcome::Unknown alongside its cause
//! and must not be automatically replayed. Request correlation IDs alone do not provide
//! deduplication. Cancellation before migration persistence leaves host records
//! unchanged; cancellation during persistence requires reconciliation.
//!
//! ## Resource lifetime
//!
//! Each storage/reader/writer resource belongs to one authenticated registration
//! and its owning WASM Store or gRPC Storage session. Resource IDs cannot be reused
//! within that scope or rebound across scopes. Proxies retain their runtime
//! independently of the catalog and MountRuntime. Unregistering prevents new routing; graceful
//! unloading stops new opens and drains retained resources before final shutdown.
//! A crash cannot preserve remote resources: old proxies fail with StaleHandle
//! or Unavailable, cached storage is invalidated, and new opens use the new
//! generation. Existing streams are never silently reopened or replayed.
//!
//! Reader EOF is sticky. Cancellation of an in-flight next call may close the
//! reader because byte consumption can be uncertain. Once execution starts, writer
//! commit and abort are terminal even on failure or cancellation; further calls
//! return SessionClosed.
//! Dropping a writer never initiates commit. If commit was already dispatched,
//! dropping its future cannot guarantee rollback. Explicit close/drop only makes
//! a best-effort release; disconnecting a gRPC Storage stream drops its entire
//! resource scope without renewal or expiry. Abandoned writes are discarded,
//! never published. A lost commit response carries MutationOutcome::Unknown. Transfers preserve backpressure and never
//! buffer an entire file merely to bridge transports.

mod driver;
mod mount_repository;
mod storage;

pub use driver::{Driver, DriverFuture};
pub use mount_repository::{MountRepository, MountRepositoryFuture};
pub use storage::validate_storage_extensions;
pub use storage::{LookupContext, Storage, StorageFileReader, StorageFileWriter, StorageFuture};

mod storage_attributes;
pub use storage_attributes::StorageAttributes;

mod open_context;
pub use open_context::OpenContext;

mod instance_provisioner;
mod instance_record_repository;
mod secret_cipher;
pub use super::host_instance_registry::record::{InstanceRecord, InstanceRecordPage};
pub use instance_provisioner::{InstanceProvisioner, PreparedConfig};
pub use instance_record_repository::InstanceRecordRepository;
pub use secret_cipher::{SecretCipher, SecretContext};

mod directory_bindings;
pub use directory_bindings::DirectoryBindings;
