//! Backend-independent storage contracts for Asset Hub.
//!
//! # Construction and boundaries
//! Requests use named constructors with explicit policies and borrowed accessors.
//! Constructors that validate addresses return `Result`. Executors can borrow
//! a request for synchronous work or for the lifetime of an async call.
//! [`AttributeRequest::into_parts`] transfers the path and action when a task
//! or protocol message needs to own them, including any attribute value buffer.
//! Optional capabilities default to unsupported and can be set on the value.
//! Addresses, names, permissions and page sizes have their own validation.
//!
//! Application ports can use these types in their interfaces, and adapters
//! implement those ports and enforce the execution contracts below. This crate
//! has no dependency on either layer. Native I/O, async execution, driver
//! identities, persistence and wire formats belong to those outer layers.
//! Optional attributes remain separate from basic metadata.
//!
//! Requests use canonical [`StoragePath`] addresses. The backend preserves
//! every symbolic link as an entry without following it, including in the
//! final slot. A namespace mismatch detected before effects returns
//! [`Rejection::Conflict`] with [`MutationOutcome::Unchanged`]. A retired
//! namespace generation returns [`BackendFailure::Stale`].
//! [`ReadRequest`] and replacement writes reject a link as
//! [`Rejection::NotRegularFile`]. [`StatRequest`], [`ReadLinkRequest`],
//! [`AttributeRequest`], non-directory removal and rename address the link
//! itself. Root is excluded from write, remove, rename and directory creation.
//! Directory creation requests create one directory under an existing parent.
//!
//! # Logical names and native storage
//! An [`EntryName`] or [`XattrName`] is identified by its original UTF-8 bytes.
//! `Report` and `report` are distinct, as are `é` and `e\u{301}`. The
//! namespace uses exact spelling for equality, hashing and ordering. A
//! [`StoragePath`] records validated names in order. Attribute keys occupy a
//! separate namespace.
//!
//! Map each visible native name to its exact logical spelling. The adapter may
//! transcode a native character representation, such as UTF-16, only losslessly
//! and without changing the spelling. This contract defines no escaping,
//! case folding, replacement characters or Unicode normalization.
//!
//! A backend with exact native lookup can use it directly. A case-insensitive
//! or normalizing backend may participate if it can obtain the authoritative
//! spelling and bind that check to the entry or key used by the operation.
//! Check each name in the address and every attribute key. A native match for
//! `report` with actual spelling `Report` is absent for exact lookup.
//! Operations that require the exact entry or key return [`Rejection::NotFound`]
//! or [`Rejection::AttributeNotFound`], respectively. If the operation would
//! create the absent spelling, the occupied native slot instead fails as
//! [`Rejection::Conflict`]. This applies to a rename to a differently spelled
//! equivalent name in the same directory, even if the equivalent slot is its
//! own source. Creation and replacement must enforce these checks atomically
//! with the name binding, including concurrent or staged mutations. An adapter
//! that cannot enforce exact lookup or safe mutation rejects the affected
//! operation as [`Rejection::Unsupported`].
//!
//! A valid logical name outside the backend's usable repertoire fails as
//! [`Rejection::UnsupportedName`], for example a reserved device name or a
//! spelling the backend would rewrite on creation. A native component or key
//! that exceeds the backend's length limit fails as [`Rejection::NameTooLong`],
//! measured in native units. Check the logical address bound before native
//! length limits. A 256-byte component can fit the logical address and still
//! fail on a backend with a 255-byte component limit. Joining individually
//! valid components into an address beyond [`MAX_PATH_BYTES`] fails as
//! [`Rejection::PathTooLong`]. A separate backend-wide address restriction
//! that cannot be satisfied by component-wise addressing is [`Rejection::Unsupported`].
//!
//! Enumeration and direct access use the same visible namespace and spelling.
//! Entries outside the exported namespace must also be outside direct lookup.
//! If a visible native entry or attribute key cannot be represented, fail the
//! page or attribute listing with [`BackendFailure::UnrepresentableName`].
//! This includes a native name containing a forbidden character or invalid
//! UTF-8. If a visible spelling decodes but is unsupported for direct access,
//! fail with the same rejection as lookup. When an individually representable
//! child would make its full address too long, fail with
//! [`Rejection::PathTooLong`]. Sort and paginate by logical spelling. Under
//! unchanged namespace and access conditions, a listed child's address
//! identifies the same slot for direct access.
//!
//! Malformed caller input fails with its value validation error. A malformed
//! logical name in a protocol claim, or an adapter returning an alias as an
//! exact match, is a contract violation. Classify by the affected response
//! as [`ContractViolation::InvalidEntry`], [`ContractViolation::InvalidPage`]
//! or [`ContractViolation::InvalidAttributes`]. These cases differ from
//! decoding an authoritative native name via [`EntryName::from_backend_bytes`]
//! or [`XattrName::from_backend_bytes`]. Logical value and page validation
//! cannot prove native spelling, atomicity or agreement with direct lookup.
//!
//! # Failure model
//! Value errors describe construction failures without guessing input provenance.
//! Validation-to-validation From conversions preserve structural nesting only.
//! Adapters explicitly classify request/response errors and retain their causes.
//! [`Failure::InvalidBinding`] identifies a caller request that disagrees with
//! its binding. [`ContractViolation`] describes defects at the adapter boundary.
//! [`StorageError`] records the reason and the effects of the whole attempt
//! independently. [`MutationOutcome::Unchanged`] requires evidence that no
//! covered change occurred or can still occur. [`MutationOutcome::Unknown`]
//! includes known partial changes and effects that cannot be ruled out.
//! Reconcile either case before replaying an unprotected mutation.
//! The adapter keeps diagnostics in the private error source. Public Display
//! and Debug expose only the operation, reason and outcome.
//!
//! | Failure scenario | Example reason | Outcome |
//! | --- | --- | --- |
//! | Operation rejected locally before dispatch | `Rejected(Unsupported)` | `Unchanged` |
//! | Backend confirms the attempt was rejected without effects | `Rejected(Conflict)` | `Unchanged` |
//! | Write staging failed and publication was never enabled | `Backend(CapacityExceeded)` | `Unchanged` |
//! | Commit failed and the backend confirms it never applied and cannot apply later | `Backend(TimedOut)` | `Unchanged` |
//! | Commit timed out or the reply was lost and publication is unresolved | `Backend(TimedOut)` or `Backend(Unavailable)` | `Unknown` |
//! | Non-atomic publication exposed a prefix before failing | `Backend(CapacityExceeded)` | `Unknown` |
//! | Partial visibility breached an atomic publication guarantee | `Contract(CapabilityMismatch)` | `Unknown` |
//! | Recursive creation left one or more visible parents before failing | `Rejected(NotDirectory)` | `Unknown` |
//! | Abort or private cleanup failed and publication is ruled out | `Backend(Unavailable)` | `Unchanged` |
//! | A read-only operation failed, with or without an incidental access-time update | `Backend(TimedOut)` | `Unchanged` |
//!
//! Choose the reason from the actual failure and use evidence of effects to
//! choose [`StorageError::without_effects`] or [`StorageError::after_dispatch`].
//! A backend guarantee must cover the correlated attempt and its earlier steps.
//! An unchanged target observed later does not establish that a delayed commit
//! cannot still publish or that no transient change was visible. A rollback
//! can justify `Unchanged` when no covered change ever became visible and the
//! attempt is fenced from applying later.
//!
//! A staged write has one logical operation identity across staging, commit,
//! abort and cleanup. The port retains the phase and any correlation needed
//! to establish effects. Private cleanup after a commit failure retains the
//! original reason and outcome. Additional cleanup diagnostics may be
//! retained in the private source. Abort and commit close the writer to
//! further calls even on failure. A dispatched commit may still complete.
//! A read-only operation that changes covered state violates the execution
//! contract. Incidental access times and automatic bookkeeping are outside
//! the scope of the outcome.
//!
//! An `Unchanged` failure leaves the retry decision to the caller's policy.
//! Check the reason, deadline, cancellation intent and current preconditions,
//! then open a fresh writer with replayable data for a retried write. For
//! `Unknown`, reconcile against the original backend generation, the
//! destination identity or expected absence, and any affected parent
//! directories before replay. A higher layer may instead supply an
//! authoritative operation receipt or an explicit deduplication guarantee.
//! The domain error carries none of those bindings or replay inputs. A
//! read-only failure may have delivered part of a stream. The caller supplies
//! the consumed offset and any identity or version needed to resume it.
//!
//! ```
//! use ah_vfs_storage::{
//!     BackendFailure, Failure, MutationOutcome, Operation, StorageError,
//! };
//! let error = StorageError::after_dispatch(
//!     Operation::Write,
//!     Failure::Backend(BackendFailure::TimedOut),
//!     Some(Box::new(std::io::Error::other("private backend diagnostic"))),
//! );
//! assert_eq!(error.outcome(), MutationOutcome::Unknown);
//! assert!(!error.to_string().contains("private backend diagnostic"));
//! ```
//!
//! # Capability declarations
//! Writes carry nonempty modes; directory guarantees carry a directory model;
//! writable attributes include reading. These combinations are valid by construction.
//! Each capability update returns a valid declaration.
//! [`MutationBinding`] declares whether final-slot checks are atomic with
//! mutation. An observed mismatch before effects is `Conflict` with
//! `Unchanged`. No-replace rename, create-new publication and single-directory
//! creation check destination absence atomically at either guarantee level.
//!
//! ```
//! use ah_vfs_storage::{
//!     Capabilities, StoragePath, Publication,
//!     WriteModes, WriteMode, WriteRequest, WriteSupport,
//! };
//! let capabilities = Capabilities::default()
//!     .with_write(WriteSupport::staged(WriteModes::CreateNew, true));
//! let request = WriteRequest::new(
//!     StoragePath::parse("documents/report.txt")?,
//!     WriteMode::CreateNew,
//!     Publication::Atomic,
//! )?;
//! capabilities.require_write(&request)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Validated values
//! ```
//! use ah_vfs_storage::{PageSize, ReadRange};
//! let range = ReadRange::new(u64::MAX, Some(1));
//! assert!(range.is_err());
//! assert_eq!(PageSize::new(20)?.get(), 20);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Directory pagination
//! Before enumerating, the executor supplies a [`DirectorySelection`]
//! established independently of the page payload. The decoder correlates the
//! response with the request and checks the page's reported address and
//! identity against the selection, even on empty pages. If the executor
//! cannot supply an independent selection, the operation is
//! [`Rejection::Unsupported`].
//!
//! A continuation carries the selected address, identity and exclusive name.
//! Resume only at the same address and incarnation in the same backend
//! generation. A selection can expire within that generation. Start a new
//! listing if resuming returns [`BackendFailure::Stale`].
//! This protects directory identity, while the contents are live. Changes
//! before the cursor may be missed and renames can move entries across it.
//! The cursor is a continuation value, rather than a stored snapshot or a
//! grant of access. The executor checks authorization on each page.
//! Producers must explicitly report whether there are more entries. Decoders
//! must explicitly supply a next name or an end marker. A full page can be
//! either final or followed by more data.
//!
//! ```
//! use ah_vfs_storage::{DirectoryIdentity, DirectoryPage, DirectorySelection,
//!     Entry, EntryType, ListRequest, PageSize, StoragePath};
//! let query = ListRequest::new(StoragePath::parse("actual")?, PageSize::DEFAULT);
//! // The executor independently obtained the directory identity.
//! let identity = DirectoryIdentity::from_bytes([1; 16]);
//! let selected = DirectorySelection::from_lookup(StoragePath::parse("actual")?, identity);
//! let bound = query.bind_selection(selected)?;
//! let entry = Entry::new(StoragePath::parse("actual/a")?, EntryType::Directory, None)?;
//! let page = DirectoryPage::new(&bound, vec![entry], true)?;
//! let (reported, entries, next) = page.into_parts();
//! let decoded = DirectoryPage::from_parts(
//!     &bound, reported.address(), reported.identity(), entries, next,
//! )?;
//! let cursor = decoded.continuation().ok_or("continuation")?;
//! assert_eq!(cursor.after().as_str(), "a");
//! let continuation = ListRequest::continuing(cursor, query.size());
//! assert!(continuation.after().is_some());
//! let resumed = continuation.bind_selection(bound.selection().clone())?;
//! assert_eq!(resumed.after().map(|name| name.as_str()), Some("a"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Omitting the producer's completion decision is a compile error.
//! ```compile_fail
//! use ah_vfs_storage::{BoundListRequest, DirectoryPage};
//! fn finish(bound: &BoundListRequest) {
//!     DirectoryPage::new(bound, vec![]);
//! }
//! ```
//!
//! Omitting the decoder's end marker is a compile error, even for an empty page.
//! ```compile_fail
//! use ah_vfs_storage::{BoundListRequest, DirectoryPage};
//! fn finish(bound: &BoundListRequest) {
//!     DirectoryPage::from_parts(bound, bound.selection().address(), bound.selection().identity(), vec![]);
//! }
//! ```
//!
//! A response needs a binding independently obtained for the request.
//! ```compile_fail
//! use ah_vfs_storage::{DirectoryIdentity, DirectoryPage, ListRequest, StoragePath};
//! fn decode(request: &ListRequest, reported: &StoragePath, identity: DirectoryIdentity) {
//!     DirectoryPage::from_parts(request, reported, identity, vec![], None);
//! }
//! ```
//!
//! # Protocol conversion
//! A protocol adapter owns its schema, stable wire tags, version negotiation,
//! framing and resource limits. Encode fields explicitly. Reconstruct domain
//! values through their public constructors and keep wire types outside this
//! crate. Apply these rules at the boundary:
//!
//! - **Timestamps.** Represent a present [`std::time::SystemTime`] as signed
//!   64-bit seconds since the Unix epoch and unsigned nanoseconds in
//!   `0..1_000_000_000`. Use floor seconds for pre-epoch values (half a
//!   second before the epoch is `(-1, 500_000_000)`). Check the wire range
//!   on encode and the host range on decode. Decode with checked epoch
//!   arithmetic, then verify the original pair is recovered on encode to
//!   catch platform precision loss. An unrepresentable present timestamp
//!   fails entry conversion as [`ContractViolation::InvalidEntry`]. Omission
//!   represents an absent timestamp. Never clamp, round or omit on failure.
//! - **Numbers and presence.** Use declared fixed-width integers. For
//!   `usize` and `NonZeroUsize`, check conversion in both directions and
//!   validate nonzero, domain bounds and protocol limits separately. Preserve
//!   absence independently of zero. For example, an absent read length means
//!   through EOF, while an explicit zero means an empty range. Validate the
//!   sum of offset and length with [`ReadRange::new`].
//! - **Tags and negotiation.** Unknown operations or required semantic tags
//!   in a request fail before dispatch. A known operation that the executor
//!   does not support is [`Rejection::Unsupported`]. Unknown tags in a
//!   response, including entry kinds, failures or mutation outcomes, are
//!   invalid responses. Unknown mandatory capabilities, unknown values in
//!   known capability fields and invalid combinations are
//!   [`ContractViolation::InvalidDeclaration`]. Ignore an unknown extension
//!   only when the negotiated schema marks it optional and independent of
//!   known guarantees. On encode, fail if the negotiated schema cannot
//!   represent a value. Rust discriminants and display strings are not tags.
//! - **Addresses.** Decode canonical addresses with [`StoragePath::parse`].
//!   The empty address denotes the backend root. Encode [`LinkTarget`] as opaque bytes.
//! - **Errors.** A storage failure carries [`Operation`], the [`Failure`]
//!   family and reason, and [`MutationOutcome`] as independent mandatory
//!   fields. Check the operation against the correlated request. Reject
//!   `Unknown` mutation outcome on a read-only operation. Preserve valid
//!   outcomes explicitly and reconstruct without a remote diagnostic source.
//!   Encode only the public fields. Error chains and formatted error text
//!   are local diagnostics. Invalid request fields fail at request decoding.
//!   An invalid reply is a contract failure, classified by the affected
//!   response (for example `InvalidPage` or `InvalidAttributes`). Once a
//!   mutation has been dispatched, an invalid reply leaves its effects
//!   unknown unless independently established.
//! - **Collections.** Decode a page with [`DirectoryPage::from_parts`]
//!   against an independently established [`BoundListRequest`]. Validate the
//!   reported address, exact identity bytes, entries and explicit end or
//!   continuation marker. Only a validated page yields a domain cursor.
//!   An incoming continuation must refer to an independently verified
//!   binding and is subject to authorization on every use. Construct an
//!   [`XattrList`] with `XattrList::new` and the original request's byte
//!   limit. Invalid order, duplicates and excessive sizes fail validation.
//!   Sorting, deduplication or truncation would conceal an invalid reply.
//! - **Allocation.** Bound the frame before buffering or decompression and
//!   independently cap decompressed output. Check declared lengths, element
//!   counts, aggregate byte budgets and integer arithmetic before allocating
//!   or copying untrusted fields. Enforce limits during decoding of repeated
//!   fields with no count prefix. Apply request-specific bounds, such as page
//!   size and attribute bytes, alongside protocol-wide ceilings. A check
//!   after a decoder has already materialized an unbounded collection cannot
//!   enforce its allocation budget.
//!
//! # Public extension boundaries
//! Operation modes, their fixed support sets and mutation outcomes are closed.
//! An added case changes what an executor must handle and breaks exhaustive matches.
//!
//! `Operation`, failure and validation categories, `EntryType`, `WriteSupport`,
//! `DirectorySupport` and `AttributeAction` are open. Callers matching those enums
//! provide a fallback. Construct extensible payloads through named functions and
//! match them with `..`. Attribute actions with a fixed permission value or
//! removal name use direct tuple variants, and their payload shapes are fixed.
//! New optional fields on extensible payloads can use defaults in existing
//! constructors. Changing a constructor signature, a fixed payload or the
//! semantics of an existing case remains a breaking change.
//!
//! ```compile_fail
//! use ah_vfs_storage::EntryType;
//! let file = EntryType::File { size: None };
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::{WriteModes, WriteSupport};
//! let writes = WriteSupport::Staged { modes: WriteModes::Both, atomic_publish: false };
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::DirectorySupport;
//! let directories = DirectorySupport::Hierarchical { create: false, persistent_empty: false };
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::DirectorySupport;
//! let directories = DirectorySupport::Prefix { create: false, persistent_empty: false };
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::AttributeAction;
//! use std::num::NonZeroUsize;
//! let action = AttributeAction::ListXattrs { max_bytes: NonZeroUsize::MIN };
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::{AttributeAction, XattrName};
//! use std::num::NonZeroUsize;
//! fn get(name: XattrName) -> AttributeAction {
//!     AttributeAction::GetXattr { name, max_bytes: NonZeroUsize::MIN }
//! }
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::{AttributeAction, XattrName, XattrWriteMode};
//! fn set(name: XattrName, value: Vec<u8>) -> AttributeAction {
//!     AttributeAction::SetXattr { name, value, mode: XattrWriteMode::Upsert }
//! }
//! ```
//!
//! Known extensible payloads require `..` in external patterns.
//! ```compile_fail
//! use ah_vfs_storage::AttributeAction;
//! fn count(action: AttributeAction) -> Option<usize> {
//!     match action { AttributeAction::ListXattrs { max_bytes } => Some(max_bytes.get()), _ => None }
//! }
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::EntryType;
//! fn size(kind: EntryType) -> Option<u64> {
//!     match kind { EntryType::File { size } => size, _ => None }
//! }
//! ```
//!
//! An open enum still requires a fallback after matching all known cases.
//! ```compile_fail
//! use ah_vfs_storage::AttributeAction;
//! fn check(action: AttributeAction) {
//!     match action {
//!         AttributeAction::GetPermissions | AttributeAction::SetPermissions(_)
//!         | AttributeAction::ListXattrs { .. } | AttributeAction::GetXattr { .. }
//!         | AttributeAction::SetXattr { .. } | AttributeAction::RemoveXattr(_) => {}
//!     }
//! }
//! ```
//!
//! Use ordinary functions and closures to compose constructors. Generated
//! builder types and state traits are outside the public API.
//! ```compile_fail
//! use ah_vfs_storage::ReadRequestBuilder;
//! ```
//!
//! ```compile_fail
//! use ah_vfs_storage::capabilities_builder::IsComplete;
//! ```
//!
//! A write request requires an explicit publication guarantee.
//! ```compile_fail
//! use ah_vfs_storage::{StoragePath, WriteMode, WriteRequest};
//! fn write(path: StoragePath) {
//!     WriteRequest::new(path, WriteMode::CreateNew);
//! }
//! ```
//!
//! A directory cannot carry a file length.
//! ```compile_fail
//! use ah_vfs_storage::EntryType;
//! let directory = EntryType::Directory { size: Some(10) };
//! ```
//!
//! Write requests reject the root address.
//! ```
//! use ah_vfs_storage::{Publication, RequestPathError, StoragePath, WriteMode, WriteRequest};
//! assert_eq!(
//!     WriteRequest::new(StoragePath::root(), WriteMode::CreateNew, Publication::Atomic),
//!     Err(RequestPathError::RootNotAllowed),
//! );
//! ```
//!
//! Private fields prevent bypassing validation.
//! ```compile_fail
//! use ah_vfs_storage::ReadRange;
//! let range = ReadRange { offset: u64::MAX, length: Some(1) };
//! ```
//!
//! Fallible constructors return a result that must be handled.
//! ```compile_fail
//! use ah_vfs_storage::ReadRange;
//! ReadRange::new(0, None).offset();
//! ```
//!
//! A value error has no implicit operational classification.
//! ```compile_fail
//! use ah_vfs_storage::{PathError, StorageError};
//! let error: StorageError = PathError::Absolute.into();
//! ```
//!
//! An operational error computes its outcome from the chosen constructor.
//! ```compile_fail
//! use ah_vfs_storage::{Failure, MutationOutcome, Operation, Rejection, StorageError};
//! let error = StorageError::after_dispatch(Operation::Write, Failure::Rejected(Rejection::Conflict), None);
//! let changed = StorageError { outcome: MutationOutcome::Unchanged, ..error };
//! ```

mod address;
mod attributes;
mod capabilities;
mod directory;
mod entry;
mod error;
mod failure;
mod link_target;
mod request;

pub use address::{EntryName, MAX_PATH_BYTES, NameError, PathError, StoragePath};
pub use attributes::{
    AttributeAction, AttributeExpectation, AttributeRequest, AttributeResult, Permissions,
    PermissionsError, XattrList, XattrListError, XattrName, XattrNameError, XattrWriteMode,
};
pub use capabilities::{
    AttributeSupport, Capabilities, DirectorySupport, MutationBinding, ReadSupport, RenameSupport,
    WriteModes, WriteSupport,
};
pub use directory::{
    BoundListRequest, DirectoryCursor, DirectoryIdentity, DirectoryPage, DirectoryPageError,
    DirectorySelection, ListRequest, PageSize, PageSizeError,
};
pub use entry::{Entry, EntryError, EntryType};
pub use error::StorageError;
pub use failure::{BackendFailure, ContractViolation, Failure, MutationOutcome, Rejection};
pub use link_target::LinkTarget;
pub use request::{
    CreateDirectoryRequest, Operation, Publication, RangeError, ReadLinkRequest, ReadRange,
    ReadRequest, RemoveKind, RemoveRequest, RenameDestination, RenameMode, RenameRequest,
    RequestPathError, StatRequest, WriteMode, WriteRequest,
};
