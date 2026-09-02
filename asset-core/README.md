# Asset Core business architecture baseline

This document is the baseline for reorganizing the Asset Hub business services before the plugin
system is redesigned. It deliberately separates current implementation facts from the target
service architecture. A target service or guarantee described here is not supported until its
runtime wiring, persistence, tests, and documentation have been changed together.

## Scope and fixed product decisions

The business kernel is centered on two independent aggregates:

- `Resource` is the aggregate root for asset identity, logical placement, kind, lifecycle,
  revision, and optional content metadata.
- `Directory` is the aggregate root for one node in the directory hierarchy. Its stable identity
  is a UUID; its path is derived from its parent chain.
- `ResourceContent` remains a value object owned by the `Resource` aggregate. It is not promoted to
  an independent aggregate merely because content operations receive their own application
  service.

The following decisions constrain the refactor:

1. A user-visible logical path must match the path in local Blob storage. This lets a user browse,
   copy, and recover managed files with an ordinary file manager while Asset Hub is not running.
2. `ResourceId` and `DirectoryId`, not paths, are stable business identities. Rename and move keep
   aggregate IDs stable while changing the logical and physical path together.
3. Resource and Directory rename/move are therefore cross-persistence workflows: they must
   coordinate aggregate persistence, the directory projection, and local storage. A database-only
   success is not a successful business operation.
4. `ContentService` becomes an independent application service, but content metadata changes are
   still made through `Resource` domain methods and persisted with Resource revision checks.
5. The existing Plugin Manifest, protocol, ABI, and authoring SDK contracts are unchanged during
   this business-service refactor. Plugin adapters may be rewired to new internal services, but the
   external contract is redesigned separately.
6. Database schemas may be changed when a better persistence boundary or recovery guarantee
   requires it. Development and test databases may be recreated on this refactor branch; schema
   compatibility must not force an inferior final model.
7. Authorization, optimistic concurrency, request idempotency, compensation, and crash recovery
   are different guarantees and must be documented independently for every write use case.

## Terms used by this document

- **Authorized surface**: an untrusted caller enters through an authorization-bound application
  facade and is checked against its current workspace.
- **Trusted surface**: runtime startup, storage reconciliation, administration, or an internal
  cross-aggregate workflow. Trusted does not mean that repository and storage invariants may be
  bypassed.
- **Revision guard**: a compare-and-swap check against an aggregate revision. It prevents a stale
  snapshot from overwriting a newer one; it is not request idempotency.
- **Request idempotency**: repeating the same logical command can be recognized and returns the
  original outcome without applying the command again.
- **Compensation**: an attempted reversal after a later step fails. In-memory compensation alone
  does not survive a process exit.
- **Recovery intent**: durable workflow state that allows startup recovery to finish or roll back a
  cross-persistence operation.
- **Convergent maintenance**: repeated reconciliation moves persisted state toward observed storage
  state. It is not a substitute for user-command idempotency.

## Current service topology

The current public application types include `ResourceService`, `SecuredResourceService`,
`DirectoryService`, `DirectoryProvisioningService`, `DirectoryIndexService`,
`SecuredDirectoryService`, `AssetCoordinator`, and `UserService`. `DirectoryServices` is a
composition-time bundle that guarantees the three Directory services share one mutation lock and
one authoritative store/index pair; it is not a fourth business service.
`ResourceCommandService`, `ResourceContentService`, `ResourceUploadService`,
`ResourceActionService`, and `StorageReconciliationService` are private borrowing wrappers around
the same `ResourceService`; they split files but do not own narrow dependency sets.

### Current Resource use cases

| Area | Current entry point | Core behavior |
| --- | --- | --- |
| Kind catalog | `ResourceService::kind_definitions`, `kind_lineage`, `describe_kind_actions` | Read frozen Host kind/action declarations. |
| Resource actions | `ResourceService::describe_resource_actions` | Filter action declarations by kind, content state, matcher, and edit size policy. |
| Find | `SecuredResourceService::find_resource` | Resolve an active Resource with its current Directory projection after workspace authorization. |
| List | `SecuredResourceService::list_resources` | Resolve a caller-relative Directory path into its workspace and page active Resources. |
| Update | `SecuredResourceService::update_resource` | Rename, move, change kind, or restore one Resource with an expected revision. |
| Soft delete | `SecuredResourceService::soft_delete_resource` | Mark a Resource deleted and move content to the internal trash path. |
| Purge | `SecuredResourceService::remove_resource` | Permanently remove the Resource record and then its Blob. |
| Generated create | internal `create_generated_resource` | Create a small Host-generated Resource and publish inline content for cross-aggregate workflows. |
| Execute action | `SecuredResourceService::execute_resource_action` | Authorize, resolve and invoke an action, validate output, then apply declared effects. |
| Runtime recovery | `pending_upload_finalizations`, `finalize_upload`, `resume_content_replacements` | Resume durable upload and content-replacement workflows. |
| Storage maintenance | `reconcile_storage*`, `scan_resources*` | Import, update, rename, verify, or remove projections according to observed local storage. |

### Current Directory use cases

| Area | Current entry point | Core behavior |
| --- | --- | --- |
| Kind catalog | `DirectoryService::kind_definitions`, `kind_lineage` | Read frozen Directory kind definitions and lineage. |
| Identity/path lookup | `root`, `find_by_id`, `locate_by_id`, `find_by_path`, `resolve_path` | Read a Directory aggregate with its current derived path. |
| Tree query | `list_children`, `list_located_children`, `contains` | Read direct children or test descendant/self containment. |
| Index maintenance | `DirectoryIndexService::rebuild`, `refresh` | Rebuild or refresh the non-authoritative in-memory Directory projection from `DirectoryStore`. |
| System provision | `DirectoryProvisioningService::provision_path` | Idempotently create missing physical and aggregate path segments for system initialization and User workspace setup. |
| Storage import | `DirectoryProvisioningService::import_storage_path` | Import a path already observed by the local storage scanner; it never serves a user Resource/Upload command. |
| Create | `DirectoryService::create`, `create_with_kind` | Create one child under a `DirectoryId` after kind, placement, sibling-name, database, index, and physical-path coordination. |
| Update/move | `DirectoryService::update` | Update one Directory and affected child kind defaults using one atomic database batch; path changes use durable relocation recovery. |
| Delete | `DirectoryService::delete_if_empty`, secured `delete` | Delete a non-root, logically and physically empty Directory from storage, database, and index. |
| Action catalog | `describe_kind_actions`, `describe_actions`, `resolve_action` | Resolve applicable Directory actions and capability providers. |
| Execute action | `AssetCoordinator::secured().execute_directory_action` | Authorize and invoke a Directory action, then apply a Directory or cross-aggregate effect. |
| Archive projection | `AssetCoordinator::secured().directory_archive_manifest` | Build an authorized point-in-time manifest for a Directory subtree archive. |

The Directory ports now have non-overlapping authority:

- `DirectoryStore` is the authoritative aggregate store. It loads aggregates, inserts one new
  aggregate, atomically applies a batch of revision updates, checks logical emptiness, and
  conditionally deletes an empty aggregate.
- `DirectoryRelocationStore` persists only unfinished rename/move intents and their desired atomic
  update batches.
- `DirectoryQuery` is a read-only tree/path projection.
- `DirectoryIndex` is only the projection writer (`replace_all`, `upsert`, `remove`).
  `DirectoryProjection` is the composition contract used when one adapter implements both narrow
  ports; business code still receives them by their separate roles.
- `DirectoryStorage` exposes exact physical existence, idempotent ensure, atomic subtree move, and
  exact empty-directory delete. It cannot recursively delete user data.

### Current Content use cases

Content is currently implemented by the private `ResourceContentService` and reached through
`SecuredResourceService` or a Resource Action.

| Area | Current entry point | Core behavior |
| --- | --- | --- |
| Read bytes | `get_resource_content` | Load the complete Blob for an active Resource. |
| Stream | `get_resource_content_stream` | Stream complete or ranged content from the path derived from Directory path and Resource name. |
| Replace stream | `replace_resource_content` | Validate revision, edit capability, size, and checksum; stage and publish content; CAS-save Resource metadata. |
| Replace action bytes | internal `replace_content_bytes_snapshot` | Apply a validated plugin `replace_content` effect through the same durable replacement workflow. |
| Recover replacement | `resume_content_replacements` | Finish cleanup for a committed replacement or restore the original Blob for an uncommitted replacement. |
| Verification | reconciliation helpers | Represent content as pending, verified, or failed and synchronize size/checksum/modified time. |

### Current Upload use cases

| Use case | Current entry point | Core behavior |
| --- | --- | --- |
| Create | `create_upload` | Resolve an existing Directory, validate kind and path availability, create staging, then save an owner-bound session. Upload never provisions paths. |
| Status | `upload_status` | Load an owner-bound session and synchronize the persisted offset with staging. |
| Append | `append_upload` | Verify requested offset and chunk checksum, append a temporary verified chunk, then CAS-advance the session offset. |
| Complete request | `complete_upload` | Move a complete session to `Finalizing`; Runtime owns dispatch and task lifetime. |
| Finalize | Runtime calls `ResourceService::finalize_upload` | Verify the whole checksum, publish the staged Blob, create Resource metadata, and mark the session completed. |
| Abort | `abort_upload` | Discard chunk/staging objects and remove the owner-bound session. |

## Current write semantics

The following tables describe current implementation behavior, including known gaps. “Retry” is
what a caller can safely assume today, not the desired final contract.

### Resource writes

| Operation | Authorization | Revision/concurrency | Physical storage | Failure compensation | Current retry semantics |
| --- | --- | --- | --- | --- | --- |
| Update metadata only | Workspace-bound `UpdateResource` | Caller revision plus repository CAS | None when name and Directory do not change | No external compensation needed | A repeated old revision conflicts; no stored request result. |
| Rename/move active Resource | Workspace-bound source; destination resolved inside the same workspace | Caller revision, ordered path locks, repository CAS | Move Blob from old logical path to new logical path without overwrite | Move Blob back if CAS/save fails | State mutation is protected, but a process exit between Blob move and Resource save has no durable recovery intent. Retry is not idempotent. |
| Restore | Workspace-bound `UpdateResource` | Caller revision and repository CAS | Move trash Blob back to the active logical path | Move it back to trash if save fails | Repeating the old request conflicts. Target path conflicts are rejected. No durable relocation intent. |
| Soft delete | Workspace-bound `DeleteResource` | Caller revision and repository CAS | Move active Blob into the Resource-ID trash path | Move Blob back if save fails | Repeating the old revision conflicts. A process exit between move and save is not recoverable from a durable intent. |
| Purge | Workspace-bound `PurgeResource` under the current coarse workspace policy | Repository conditional remove using the loaded revision | Delete the current active/trash Blob after record deletion | No durable cleanup intent; a Blob delete failure can leave an orphan after the record is gone | Repository and Blob delete primitives are individually repeatable, but the application result is not durably recorded. |
| Host-generated create | Trusted cross-aggregate workflow | Path lock and duplicate-path query; new UUID; repository `save` | Stage, publish-if-absent, then save Resource | Delete published Blob if repository save fails | Repeating creates a new UUID or conflicts on path; no request idempotency key. |
| Resource action effect | User authorized for execute or delete; plugin permissions are checked by the executor boundary, with Host grants additionally required only for grant-gated permissions | Write actions require expected revision; effects use Resource CAS workflows | Replacement or trash move according to effect | Delegates to content replacement or soft-delete compensation | Invocation has no durable command ID; retry can execute plugin code again even when a stale revision prevents a second Host mutation. |

### Directory writes

| Operation | Authorization | Revision/concurrency | Physical storage | Failure compensation | Current retry semantics |
| --- | --- | --- | --- | --- | --- |
| Provision path | Trusted User workspace/system initialization only | Shared Directory mutation lock; no caller revision | Create missing physical segments before their aggregate records | A newly created physical segment is deleted if its insert fails; a pre-existing path is never deleted as compensation | Existing paths converge to the same UUID projection; no user-facing command result is stored. |
| Create child | Workspace-bound `CreateDirectory` for secured callers | Shared mutation lock; ordinary create has no expected parent revision; action create checks the captured parent revision | Ensure the physical child before insert | Delete the physical child only when this attempt created it and the insert fails; index failures trigger rebuild | Repeating conflicts on sibling path; no command ID or original-result replay. |
| Change kind without path change | Workspace-bound source | Caller revision and one atomic `DirectoryStore::update_batch_if_unchanged` transaction for parent and automatic direct-child kind updates | No physical move | Database batch is all-or-none; index is rebuilt from the committed store | Old revisions conflict; retry requires a fresh revision. |
| Rename/move | Workspace-bound source and destination | Caller revision, shared mutation lock, and the same atomic batch | Persist relocation intent, atomically rename the complete local subtree, atomically commit the database batch, rebuild index, clear intent | A CAS conflict rolls the physical path back; transient database/index failure retains the intent; Runtime startup resumes forward from source or destination state | A returned success is complete. An interruption after intent persistence is deterministically completed on startup while preserving UUID. |
| Delete empty | Workspace-bound `DeleteDirectory` | Caller revision, authoritative empty check, then conditional database delete | Idempotently remove exactly the empty physical Directory; never recurse or remove ancestors | If the conditional database delete loses a race or fails, recreate the physical Directory. A crash after physical deletion is completed by missing-directory reconciliation rather than re-imported | Final state is convergent and the deleted physical path cannot resurrect its aggregate; response replay is not stored. |
| Directory action effect | User authorized for execute or delete; plugin permissions apply, with Host grants additionally required only for grant-gated permissions | Write actions require expected revision; create child also checks captured parent revision | Depends on create/update/delete effect | Delegates to Directory workflows | Invocation has no durable command ID. |
| Create tree | Workspace-bound Directory action plus plugin permissions | Root revision checked before application; each created aggregate has its own write | Create physical directories and publish generated Resource Blobs | Best-effort reverse removal; rollback failures are logged; no durable workflow intent | Retry can conflict or create a partial second result after interruption. |

### Content writes

| Operation | Authorization | Revision/concurrency | Physical storage | Failure compensation/recovery | Current retry semantics |
| --- | --- | --- | --- | --- | --- |
| Streamed content replacement | Workspace-bound `ReplaceResourceContent`; currently also requires a discovered plugin `edit` capability | Caller revision, target-path lock, Resource re-query, repository CAS | Stage new bytes, move target to backup, publish staged bytes at the same logical path | Durable replacement intent is written before target mutation; startup cleans a committed result or restores an unchanged Resource | Crash recovery is strong, but repeating the original request after success conflicts on revision; there is no request idempotency key. |
| Plugin replacement effect | User authorization plus the plugin manifest `resource.content.replace` permission | Action expected revision and the same Content replacement CAS | Same as streamed replacement | Same durable replacement intent | Plugin invocation may repeat; Host mutation is revision guarded but the original result is not replayed. |
| Reconciliation metadata update | Trusted maintenance | Resource CAS | Reads the existing physical Blob; normally does not move it | A failed or incomplete scan does not delete unseen Resources; per-key failures are surfaced or marked failed | Convergent maintenance, not request idempotency. |

### Upload writes

| Operation | Authorization | Revision/concurrency | Physical storage | Failure compensation/recovery | Current retry semantics |
| --- | --- | --- | --- | --- | --- |
| Create upload | Authenticated user; Directory path is resolved inside the workspace; the session stores owner ID | New Upload UUID and per-session lock | Create an empty internal staging object | Discard staging if session save fails | Repeating creates another session/staging object; no idempotency key. |
| Append chunk | Session owner only | Per-upload lock plus expected offset CAS | Write and checksum a temporary chunk, then append it at the expected staging offset | Temporary chunk is discarded; session offset can resynchronize from staged length | Offset prevents duplicate bytes, but retry after an unknown response returns an offset conflict rather than the original success. This is resumable, not request-idempotent. |
| Request finalization | Session owner only | Conditional transition to `Finalizing` | Discard temporary chunk; keep complete staging | Runtime can rediscover `Finalizing` sessions | Repeating while `Finalizing` or `Completed` returns the current session and does not dispatch a second transition. |
| Finalize upload | Trusted Runtime for an owner-approved session | Per-upload and target-path locks; Resource/path duplicate checks | Publish staging if absent and verify target size/checksum | Detect a previously published matching target; remove Resource or target on later failures; pending finalizations are recovered on startup | Designed for recovery and repeated execution, although cleanup failures are not represented by a general command result. |
| Abort | Session owner only | Per-upload lock | Idempotent staging/chunk discard | Remove session after storage cleanup | A second call returns not found because the session record is gone; final state is convergent but response replay is absent. |

### Storage maintenance writes

| Operation | Authorization | Revision/concurrency | Physical storage | Failure compensation/recovery | Current retry semantics |
| --- | --- | --- | --- | --- | --- |
| Import observed Directory | Trusted maintenance through `DirectoryProvisioningService` | Shared Directory mutation lock; no caller revision | The exact physical Directory must already exist | Insert missing aggregates and refresh/rebuild the index; the observed physical path remains available for retry | Convergent; Resource and Upload business commands cannot call this capability. |
| Import observed Blob | Trusted maintenance | Per-path lock; Resource insert/upsert according to the observed path | Reads the existing Blob and records metadata before background verification | A later scan can rediscover the same path; invalid or failed verification is represented explicitly | Convergent by logical path, without a request result. |
| Refresh changed Blob | Trusted maintenance | Per-path lock and Resource CAS | Reads size, modification time, and bytes for checksum | Verification failure is persisted when possible; storage errors remain visible | Repeated scans converge when the physical Blob stops changing. |
| Reconcile confirmed rename | Trusted maintenance | Ordered source/target path locks and Resource CAS | Physical rename has already occurred; metadata is moved to the observed target path while preserving Resource ID when unambiguous | Falls back to per-key reconciliation when the target is missing; a CAS conflict is surfaced | Convergent after a stable filesystem event sequence. |
| Remove missing-Blob Resource | Trusted maintenance | Per-path lock and conditional Resource remove | Physical Blob is already absent | Incomplete scans do not perform unseen-object deletion; removal is conditional on a second inspect | Repeated scans converge. |
| Remove missing Directory projection | Trusted maintenance | Directory mutation lock and repository empty check | Physical Directory is already absent | Non-empty aggregate directories are retained; the index follows a successful repository removal | Repeated full scans converge from deepest Directory to root. |

## Known baseline problems to resolve

1. `ResourceService` owns unrelated upload, action, recovery, scanner, and health dependencies.
2. Private Resource subservices borrow the entire `ResourceService`, so file separation has not
   produced dependency separation.
3. Content replacement is incorrectly coupled to discovery of a plugin-provided `edit` capability.
   Content business validity and UI/editor availability must be separate decisions.
4. Resource path relocation still uses immediate compensation but has no durable recovery intent.
5. `ListResources` carries mutually exclusive path and Directory-ID filters. Core queries should
   use stable IDs after an authorized boundary resolves a path.
6. Ordinary Resource stores expose unconditional upsert/delete operations needed by maintenance and
   compensation, making it too easy for a business service to bypass revision rules.
7. Plugin action invocation and externally retried create/replace commands have no durable command
   identity or result replay.

## Target service architecture

### ResourceService

Owns Resource metadata and lifecycle use cases:

- get/list Resource projections;
- update name, Directory, and kind;
- restore and soft delete;
- enforce Resource invariants and revision checks;
- coordinate durable logical/physical relocation when name, Directory, restore, or soft-delete
  changes the local storage path.

It does not own Blob streaming, uploads, plugin executors, storage scanning, or runtime recovery.

### ContentService

Owns Resource content use cases:

- read full content;
- stream full/ranged content;
- replace content;
- verify size and checksum;
- persist and recover content replacement intents.

It has a narrow Resource store dependency because it must call Resource domain methods and CAS-save
content metadata. It does not depend on `ResourceService`, plugin capability discovery, Directory
mutation, upload sessions, or storage scanning.

### DirectoryService

Owns normal Directory query and lifecycle use cases:

- resolve stable identity and current path projections;
- list and test tree containment;
- create, update, move, and delete Directory aggregates;
- enforce kind placement, cycle, sibling-name, root, and revision invariants;
- coordinate the local physical directory for normal Directory mutations.

Trusted recursive provisioning and storage import are separate capabilities rather than ordinary
Directory business methods.

### DirectoryProvisioningService

Owns only trusted path materialization:

- `provision_path` for system initialization and User workspace setup;
- `import_storage_path` for paths already observed by storage reconciliation.

Neither Resource update nor Upload creation can obtain this service. Their destination must already
resolve through `DirectoryService`.

### DirectoryIndexService

Owns the rebuildable Directory query projection:

- `rebuild` replaces the complete projection from `DirectoryStore`;
- `refresh` updates one projection and falls back to rebuild on adapter failure.

The index is never authoritative. Directory writes commit the store/physical workflow before
publishing the index result.

### UploadService

Owns the UploadSession state machine and ingestion workflow:

- create/status/append/complete/abort;
- finalization and startup discovery;
- owner checks, offsets, chunk and full checksums, staging, and target publication.

It collaborates with Directory query, Resource storage, and content-storage capabilities without
being a child facade of `ResourceService`.

### ActionOrchestrator

Owns Host-side action orchestration:

- describe/resolve an applicable action;
- invoke the configured action executor;
- validate declared access, views, effects, invocation identity, and output limits;
- translate validated effects into explicit Resource, Content, Directory, or Asset workflow
  commands.

It never mutates repositories or Blob storage directly. The current external Plugin ABI remains an
adapter boundary around this orchestrator until the plugin redesign starts.

### AssetWorkflowService

Owns workflows that span Resource and Directory aggregates, including:

- bounded create-tree application;
- archive manifests;
- future copy/import/batch workflows.

Every multi-write workflow must define its atomicity, durable intent or explicitly documented
partial-result semantics, authorization scope, and retry contract.

### StorageMaintenanceService

Owns trusted reconciliation and recovery driven by local storage facts:

- startup reconciliation;
- full scans and progress;
- per-key and confirmed-rename reconciliation;
- verification refresh;
- storage import and missing-object cleanup;
- Directory projection/index rebuild support.

It uses maintenance-only persistence capabilities and explicit Directory provisioning. These
capabilities are not exposed through authorized user services.

## Target dependency direction

```text
HTTP / CLI / Runtime / Storage watcher / Plugin adapters
                        |
                        v
Authorization-bound facades and workflow entry points
                        |
        +---------------+----------------+
        |               |                |
        v               v                v
ResourceService   ContentService   DirectoryService
        ^               ^                ^
        |               |                |
        +------- UploadService ----------+
        +---- ActionOrchestrator --------+
        +---- AssetWorkflowService ------+
        +-- StorageMaintenanceService ---+
                        |
                        v
          narrow semantic ports/adapters
                        |
                        v
      SQLite / local Blob storage / indexes
```

Application services may collaborate, but a service must not obtain another service's entire
dependency graph merely for convenience. Cross-aggregate mutation belongs to an explicit workflow
service. Domain aggregates never depend on services or ports.

## Rules for subsequent refactor steps

For every changed or new write use case, its implementation and documentation must answer:

1. What aggregate or workflow owns the decision?
2. Is the caller authorized, trusted maintenance, or both through separate entry points?
3. What stable ID identifies the target, and where is any path resolved?
4. What revision, offset, or other concurrency precondition is required?
5. What database and local-storage writes occur, and in what order?
6. Which failure steps are compensated immediately?
7. Which interruption points require a durable recovery intent?
8. Is retry naturally idempotent, revision-guarded, resumable, convergent, or backed by a durable
   idempotency key and stored result?
9. Which smallest semantic ports and atomic guarantees does the use case require?
10. Which high-risk invariant or recovery ordering must remain executable context in a test?

Do not add compatibility facades, deprecated aliases, or parallel old/new service graphs during
this branch. Each refactor step migrates all in-repository callers, removes the replaced surface,
updates affected documentation, and returns the workspace to a compiling and tested state.

## Baseline validation

The baseline command for this architecture refactor is:

```bash
cargo test --workspace
```

The result recorded for this document must reflect an actual run on the refactor branch. Passing
tests establish only that the documented current implementation still satisfies its executable
invariants; they do not convert the target architecture or known gaps into implemented behavior.

Baseline recorded on 2026-09-01 for branch `before_plugin/adjust_business_code`:

- `cargo test --workspace`
- 204 tests passed
- 0 failed, 0 ignored
