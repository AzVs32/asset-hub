# Asset Plugin API

`asset-plugin-api` is the source of truth for Asset Hub's language-neutral external plugin
contract. Rust types and typed catalogs in this crate generate the committed JSON Schema Draft
2020-12 documents, Browser Frame catalog, Extism Host ABI catalog, and Web SDK constants under
[`spec/`](spec/). It does not contain a Wasm runtime adapter or a plugin authoring facade.

Rust plugin authors use `asset-rust-sdk`; Host adapters and implementations of other language SDKs
depend on this crate directly.

The current, only supported contract is Manifest v5 / Plugin API v4. This is an intentionally
closed protocol: unknown fields are rejected in Manifest, lock, Action JSON, ABI JSON, and Frame
output documents. A contract extension therefore requires a version change rather than silently
shipping fields that older consumers ignore.

Packages built for Plugin API v3 are not loaded. Rebuild and reinstall them with a v4 SDK and
Manifest; the Host intentionally provides no v3 compatibility path.

The public Rust API uses flat, curated `manifest`, `protocol`, and `abi` exports. Their internal
source files are organization details, not public module paths. The contract uses two complementary
naming axes:

- Resource and Directory Action messages are aggregate-bound JSON protocols.
- Content and Directory Host functions are capabilities callable by Wasm guests.

Browser Frame channels form one protocol family while remaining aggregate-specific capability
surfaces: `asset-hub.plugin-frame.resource@4` and `asset-hub.plugin-frame.directory@4`. The
language-neutral catalog groups each channel with its Host methods under
`browser_frames.resource` and `browser_frames.directory`.

The content ABI is deliberately capability-named rather than aggregate-named. It reads opaque,
call-scoped content references issued either directly to a Resource Action or through Resource
entries listed by a Directory Action. It does not expose Resource identity, metadata, mutation, or
persistence as a Host function surface.

Resource snapshots expose one authoritative `state` object with lifecycle, content, and effective
states. The lifecycle deletion timestamp exists only in the `deleted` variant. Content metadata
does not repeat verification state, and plugins must not infer effective state from metadata or
timestamps.

Manifest loading is two-phase. Deserialize untrusted JSON as `PluginManifestDocument`, then consume
it with `validate()` to obtain `ValidatedPluginManifest`. Host catalogs and package verification
accept only the validated type. Validation failures expose a stable code, JSON-path-like location,
and human-readable message. Every Resource and Directory Action ID must be in the declaring
plugin's namespace: `${plugin.id}.…`.

Lock integrity keys use `PluginPackagePath`, a canonical relative UTF-8 package path with `/`
separators. They are never native `Path`/`PathBuf` values; filesystem adapters perform the explicit
conversion at their boundary.

Regenerate committed artifacts and the Web SDK catalog with:

```bash
cargo run -p asset-plugin-api --example export_contract -- \
  asset-plugin-api/spec sdk/asset-web-sdk/src/contract.generated.ts
```

Contract tests fail when either generated output drifts from the Rust definitions.
