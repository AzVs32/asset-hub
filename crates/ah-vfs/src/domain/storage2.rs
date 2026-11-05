//! Independent second-generation storage contracts, intended for a future migration.
//!
//! # Construction and boundaries
//! Composite values use bon builders. Required members are checked at compile time;
//! optional capabilities default to unsupported. Fallible method builders execute
//! the same validated constructor as their direct new() entry points. Finishing
//! these builders returns Result; no unchecked field builder is exposed.
//! Scalar addresses, names, permissions and page sizes retain parse/new/TryFrom.
//!
//! Native I/O, async ports, driver identities, persistence and wire formats belong
//! outside this module. Neither the old storage contract nor application services
//! are imported. Names are exact UTF-8; adapters enforce backend limits and confinement.
//! Stat uses StoragePath; removal uses NonRootPath (file, link itself or empty dir).
//! Optional attributes remain separate from basic metadata.
//!
//! # Failure model
//! Value errors describe construction failures without guessing input provenance.
//! Validation-to-validation From conversions preserve structural nesting only.
//! Adapters explicitly classify request/response errors and retain their causes.
//! Operational errors distinguish why the operation failed from whether a mutation
//! may have taken effect. No reason implies automatic retry or confirmed rollback.
//! Public Display/Debug never include the private diagnostic source.
//!
//! ```
//! use ah_vfs::domain::storage2::{
//!     BackendFailure, Failure, MutationOutcome, Operation, StorageError,
//! };
//! let error = StorageError::after_dispatch()
//!     .operation(Operation::Write)
//!     .failure(Failure::Backend(BackendFailure::TimedOut))
//!     .source(std::io::Error::other("private backend diagnostic"))
//!     .build();
//! assert_eq!(error.outcome(), MutationOutcome::Unknown);
//! assert!(!error.to_string().contains("private backend diagnostic"));
//! ```
//!
//! # Capability declarations
//! Writes carry nonempty modes; directory guarantees carry a directory model;
//! writable attributes include reading. These combinations are valid by construction.
//! No separate validate() step is required after building the declaration.
//!
//! ```
//! use ah_vfs::domain::storage2::{
//!     Capabilities, NonRootPath, Publication, StoragePath,
//!     WriteModes, WriteMode, WriteRequest, WriteSupport,
//! };
//! let capabilities = Capabilities::builder()
//!     .write(WriteSupport::staged().modes(WriteModes::CreateNew).atomic_publish(true).build())
//!     .build();
//! let request = WriteRequest::builder()
//!     .path(NonRootPath::try_from(StoragePath::parse("documents/report.txt")?)?)
//!     .mode(WriteMode::CreateNew)
//!     .publication(Publication::Atomic)
//!     .build();
//! capabilities.require_write(&request)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Fallible builders
//! ```
//! use ah_vfs::domain::storage2::{ListRequest, PageSize, ReadRange, StoragePath};
//! let request = ListRequest::builder()
//!     .directory(StoragePath::parse("documents")?)
//!     .size(PageSize::new(20)?)
//!     .build()?;
//! let range = ReadRange::builder().offset(u64::MAX).length(1).build();
//! assert!(range.is_err());
//! assert_eq!(request.size().get(), 20);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Pagination completion
//! Every producer must explicitly choose has_more(true/false). Decoders must
//! supply next(Some(name)) or next(None); missing information is never treated
//! as an implicit end. A full page can be either final or followed by more data.
//!
//! ```
//! use ah_vfs::domain::storage2::{DirectoryPage, Entry, EntryType, ListRequest, StoragePath};
//! let query = ListRequest::builder().directory(StoragePath::root()).build()?;
//! let entry = Entry::builder().path(StoragePath::parse("a")?).entry_type(EntryType::Directory).build();
//! let page = DirectoryPage::builder().request(&query).entries(vec![entry]).has_more(true).build()?;
//! let (entries, next) = page.into_parts();
//! let decoded = DirectoryPage::from_parts().request(&query).entries(entries).next(next).build()?;
//! assert_eq!(decoded.next_after().map(|name| name.as_str()), Some("a"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Omitting the producer's completion decision is a compile error:
//! ```compile_fail
//! use ah_vfs::domain::storage2::{DirectoryPage, ListRequest, StoragePath};
//! let query = ListRequest::builder().directory(StoragePath::root()).build()?;
//! DirectoryPage::builder().request(&query).entries(vec![]).build();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Omitting the decoder's continuation is also a compile error, including empty pages:
//! ```compile_fail
//! use ah_vfs::domain::storage2::{DirectoryPage, ListRequest, StoragePath};
//! let query = ListRequest::builder().directory(StoragePath::root()).build()?;
//! DirectoryPage::from_parts().request(&query).entries(vec![]).build();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Extension boundaries
//! Capability admission exhaustively handles known modes and guarantees. Adding
//! a new mode requires reviewing these matches; it cannot inherit Both or Atomic.
//! Extensible payload variants are constructed through builders and matched with
//! `..`, leaving room for optional fields without breaking external callers.
//!
//! ```compile_fail
//! use ah_vfs::domain::storage2::EntryType;
//! let file = EntryType::File { size: None };
//! ```
//!
//! ```compile_fail
//! use ah_vfs::domain::storage2::{WriteModes, WriteSupport};
//! let writes = WriteSupport::Staged { modes: WriteModes::Both, atomic_publish: false };
//! ```
//!
//! ```compile_fail
//! use ah_vfs::domain::storage2::DirectorySupport;
//! let directories = DirectorySupport::Hierarchical { create: false, persistent_empty: false };
//! ```
//!
//! ```compile_fail
//! use ah_vfs::domain::storage2::DirectorySupport;
//! let directories = DirectorySupport::Prefix { create: false, persistent_empty: false };
//! ```
//!
//! Known payload variants cannot be matched exhaustively outside this crate:
//! ```compile_fail
//! use ah_vfs::domain::storage2::EntryType;
//! fn size(kind: EntryType) -> Option<u64> {
//!     match kind { EntryType::File { size } => size, _ => None }
//! }
//! ```
//!
//! Atomic publication requires explicit supported write modes:
//! ```compile_fail
//! use ah_vfs::domain::storage2::WriteSupport;
//! WriteSupport::staged().atomic_publish(true).build();
//! ```
//!
//! A request must explicitly select its publication guarantee:
//! ```compile_fail
//! use ah_vfs::domain::storage2::{NonRootPath, StoragePath, WriteMode, WriteRequest};
//! let path = NonRootPath::try_from(StoragePath::parse("file")?)?;
//! WriteRequest::builder().path(path).mode(WriteMode::CreateNew).build();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! A directory cannot carry a file length:
//! ```compile_fail
//! use ah_vfs::domain::storage2::EntryType;
//! let directory = EntryType::Directory { size: Some(10) };
//! ```
//!
//! A root address cannot be submitted as a write target:
//! ```compile_fail
//! use ah_vfs::domain::storage2::{Publication, StoragePath, WriteMode, WriteRequest};
//! WriteRequest::builder().path(StoragePath::root())
//!     .mode(WriteMode::CreateNew).publication(Publication::Atomic).build();
//! ```
//!
//! Private fields prevent bypassing construction checks:
//! ```compile_fail
//! use ah_vfs::domain::storage2::ReadRange;
//! let range = ReadRange { offset: u64::MAX, length: Some(1) };
//! ```
//!
//! Fallible builder results require handling before accessing the domain value:
//! ```compile_fail
//! use ah_vfs::domain::storage2::ReadRange;
//! ReadRange::builder().offset(0).build().offset();
//! ```
//!
//! A value error has no implicit operational classification:
//! ```compile_fail
//! use ah_vfs::domain::storage2::{PathError, StorageError};
//! let error: StorageError = PathError::Absolute.into();
//! ```
//!
//! An error builder computes its outcome rather than accepting an override:
//! ```compile_fail
//! use ah_vfs::domain::storage2::{Failure, MutationOutcome, Operation, Rejection, StorageError};
//! StorageError::after_dispatch().operation(Operation::Write)
//!     .failure(Failure::Rejected(Rejection::Conflict))
//!     .outcome(MutationOutcome::Unchanged).build();
//! ```
//!
//! Setting a cause twice is rejected instead of silently replacing diagnostics:
//! ```compile_fail
//! use ah_vfs::domain::storage2::{Failure, Operation, Rejection, StorageError};
//! StorageError::without_effects().operation(Operation::Write)
//!     .failure(Failure::Rejected(Rejection::Unsupported))
//!     .source(std::io::Error::other("first cause"))
//!     .source(std::io::Error::other("replacement cause")).build();
//! ```

mod address;
mod attributes;
mod capabilities;
mod directory;
mod entry;
mod error;
mod failure;
mod request;

pub use address::{
    EntryName, MAX_PATH_BYTES, NameError, NonRootPath, PathError, RootPathError, StoragePath,
};
pub use attributes::{
    Permissions, PermissionsError, XattrList, XattrListBuilder, XattrListError, XattrName,
    XattrNameError, XattrWriteMode, xattr_list_builder,
};
pub use capabilities::{
    AttributeSupport, Capabilities, CapabilitiesBuilder, DirectorySupport,
    DirectorySupportHierarchicalBuilder, DirectorySupportPrefixBuilder, ReadSupport, RenameSupport,
    WriteModes, WriteSupport, WriteSupportStagedBuilder, capabilities_builder,
    directory_support_hierarchical_builder, directory_support_prefix_builder,
    write_support_staged_builder,
};
pub use directory::{
    DirectoryPage, DirectoryPageBuilder, DirectoryPageError, DirectoryPageFromPartsBuilder,
    ListRequest, ListRequestBuilder, PageSize, PageSizeError, directory_page_builder,
    directory_page_from_parts_builder, list_request_builder,
};
pub use entry::{
    Entry, EntryBuilder, EntryType, EntryTypeFileBuilder, entry_builder, entry_type_file_builder,
};
pub use error::{
    StorageError, StorageErrorAfterDispatchBuilder, StorageErrorWithoutEffectsBuilder,
    storage_error_after_dispatch_builder, storage_error_without_effects_builder,
};
pub use failure::{BackendFailure, ContractViolation, Failure, MutationOutcome, Rejection};
pub use request::{
    CreateDirectoryMode, CreateDirectoryRequest, CreateDirectoryRequestBuilder, Operation,
    Publication, RangeError, ReadRange, ReadRangeBuilder, ReadRequest, ReadRequestBuilder,
    RenameMode, RenameRequest, RenameRequestBuilder, WriteMode, WriteRequest, WriteRequestBuilder,
    create_directory_request_builder, read_range_builder, read_request_builder,
    rename_request_builder, write_request_builder,
};
