# Asset Rust SDK

`asset-rust-sdk` is the supported high-level authoring SDK for Rust plugins. Plugin business code
imports contexts, bounded readers, response builders, diagnostics, the SDK-owned `Error`/`Result`,
and export macros directly from the crate root.

The runtime-neutral authoring surface is available without selecting an adapter. Enable
`extism-guest` when building a plugin for the currently implemented Extism Host integration.

```toml
[dependencies]
asset-rust-sdk = { path = "<asset-hub>/sdk/asset-rust-sdk", features = ["extism-guest"] }
```

```rust
use asset_rust_sdk::{
    Media, ResourceContext, ResourceResponse, Result, export_resource_action,
};

export_resource_action!(render_thumbnail => render_thumbnail_action);

fn render_thumbnail_action(context: ResourceContext) -> Result<ResourceResponse> {
    Ok(ResourceResponse::media(
        Media::base64("image/svg+xml", b"<svg/>").title(context.resource().name()),
    ))
}
```

The SDK depends on `asset-plugin-api`, but does not re-export its Manifest or wire DTO modules.
Host adapters and implementations of other language SDKs depend on `asset-plugin-api` directly.
The current `extism-guest` feature is an adapter implementation; public handler errors remain
independent from Extism so additional Wasm runtimes can implement the same authoring API.

Run checks from the repository root:

```bash
cargo test -p asset-rust-sdk --all-features
```
