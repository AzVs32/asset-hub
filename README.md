# Asset Hub

[简体中文](README.zh.md)

## Workspaces

The VFS project contains the `crates/ah-vfs` core and two official driver plugins:
`crates/vfs-driver-local` and `crates/vfs-driver-memory`. The shared `crates/vfs-driver-kit` supplies typed SQLite instance stores, IDs and
error classification. These four crates and the unpublished, test-only
`crates/vfs-integration-tests` belong to the root workspace and share its package
settings, lints, `Cargo.lock` and `target/`. Local and Memory do not depend on each
other, including in their tests. Each runs the shared `ah-vfs` storage and registry
contract assertions independently; cross-driver service and mixed-mount scenarios
live in `vfs-integration-tests`, which depends on both drivers only for testing.
The root commands validate all five crates independently of optional development tools:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
```

The VFS CI workflow runs these checks on Linux when the crates, root manifests or
workflow change. Hosts supply database pools and register the official Rust driver
plugins in `DriverCatalog`. LocalDriver stores files in a local directory.
MemoryDriver persists instance settings in `memory_instances`, including
`capacity_size` and `capacity_unit` (`B`, `KiB`, `MiB`, `GiB` or `TiB`). Its file
contents are volatile: opens on the same driver object share contents, while a
new driver object starts empty even when it reuses the same settings database.

## Instance records

`Driver` declares immutable creation metadata in `DriverInfo`, selects its
`InstanceManagement`, and opens storage using `OpenContext`. Local and Memory use
self-managed `InstanceRegistry` ports; their existing SQLite tables remain unchanged.
`InstanceService` handles public instance operations and mount-reference checks.
The `testing` feature of `ah-vfs` exposes a reusable registry contract suite.

For `HostManaged` drivers, supply an `InstanceProvisioner` and configure
`MountRuntime::with_host_instances` with an `InstanceRecordRepository` and an
optional `SecretCipher`. Host records retain schema `config_version` separately
from the incrementing `revision`; updates preserve `x-immutable` fields and may
update instances referenced by mounts. Storage opens receive decrypted parameters,
with per-instance schema migration before opening. Queries do not open storage.
`x-secret` values are encrypted before persistence and public details/Debug are
redacted. Secret-bearing schemas reject creation without a cipher. No built-in
cipher is provided; the host owns authenticated encryption, keys and nonces.
Annotation traversal supports nested properties, array items and schema-valued
additionalProperties; unsupported references/combinators fail closed. Configuration
inputs use the immutable, redacted `InstanceConfig` wrapper; maps convert with `.into()`.

## Driver initialization settings

Driver-wide `DriverSettings` are separate from host settings and per-instance
`InstanceConfig`. Optional `DriverInfo::settings_schema()` borrows immutable JSON
Schema metadata with `x-secret` annotations. Await
`DriverCatalog::register(driver, settings)` to call `initialize` before registration;
duplicate keys are rejected before initialization, and failure/cancellation never
registers the provider. The catalog does not retain settings. Already-assembled
built-ins accept empty settings through the default initializer; providers with
settings must implement initialization and may connect to their own database,
including PostgreSQL, without receiving a host SQLite pool.
The configuration namespace marker is now `DriverConfigNamespace`; its TOML key
remains `vfs.driver`, including existing `vfs.driver.local` sections.

## VFS entry access

The three VFS services share one `Arc<MountRuntime>`. It retains the validated
mount table and caches validated storage handles by `(DriverKey, InstanceId)`.
Mount changes invalidate the table; instance removal invalidates its handle,
including uncertain or cancelled writes. Driver opening remains a cold path.
External persistence changes require `MountRuntime::invalidate()`; external
instance configuration changes can use `invalidate_instance()`. Invalidation
releases cached handles without revoking operations or streams already in flight.
EntryService resolves the deepest mount from the current runtime snapshot.
It protects mount points and directories containing mounts from file mutations,
and rejects cross-mount moves, even when two mounts reference the same instance.
`EntryService::remove_entry` removes files, links themselves and empty directories only. Symlinks are not
followed. Open streams retain their original backend; changes to mounts do not
revoke existing streams. Namespace changes are not serialized with filesystem I/O.

## Storage capabilities and errors

`DriverInfo::capabilities()` declares an implementation's capability upper bound;
`Storage::capabilities()` reports support for the opened instance and platform.
Declarations cover sequential/range reads, staged create/replace writes, atomic
publication, directory representation and empty-directory persistence, atomic
rename with/without replacement, and typed storage attributes. They describe
supported semantics, not caller authorization or guaranteed success on every path
or filesystem. EntryService validates declarations and rejects unsupported requests
before invoking the backend. Prefix-based adapters must define empty-directory and
file/prefix collision behavior; copy/delete cannot satisfy atomic rename.

`StorageWriteFileRequest::require_atomic_publish` and the corresponding VFS request
field explicitly require atomic complete-file publication. Unsupported guarantees
fail before staging. Atomic publication does not promise power-loss durability.
`StoragePermissions` exposes validated POSIX mode bits; `StorageXattrs` exposes
opaque, size-bounded values with atomic create/replace policies. LocalDriver
implements permissions on Unix and xattrs on Linux/Android (subject to filesystem
support). Attribute operations use pinned handles, never follow symlinks, and
require enough access to open the target for reading. VFS attribute mutations
preserve mount/ancestor protection. Other platforms advertise unsupported extensions.

The authoritative `domain::{StorageError, StorageErrorKind, DriverError}` model is
shared by instance opening and I/O; import these types directly from `domain`.
Inspect `StorageError::kind()` or `EntryError::storage_error()` instead of
matching backend SDK types or strings. Original causes remain in `Error::source()`
for trusted diagnostics; Display exposes only normalized categories. Missing
entries, stale handles, unavailable backends, timeouts, conflicts and unknown
publication outcomes remain distinct. No category implies an automatic retry.
Driver implementations supply these mappings at their adapter boundaries. The
concrete official driver plugins are LocalDriver and MemoryDriver. No RPC/WASM
adapters are currently provided.

## License

Copyright 2026 AzVs. This project is licensed under the [Apache License 2.0](LICENSE). You may use, modify, and distribute it under the terms of that license. It includes an express patent grant and is provided without warranties or conditions unless required by applicable law.
