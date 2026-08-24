# Asset Plugin API

`asset-plugin-api` is the canonical Rust representation of Asset Hub's language-neutral external
plugin contract. It owns Manifest documents, Action JSON protocols, diagnostics, effects, views,
and Wasm Host ABI names and value objects. It does not contain a Wasm runtime adapter or a plugin
authoring facade.

Rust plugin authors use `asset-rust-sdk`; Host adapters and implementations of other language SDKs
depend on this crate directly.
