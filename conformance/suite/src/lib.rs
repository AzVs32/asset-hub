//! Independent conformance workspace; production workspace membership is unchanged.
//!
//! From the repository root:
//! ```text
//! CARGO_TARGET_DIR=target cargo build --offline --release --manifest-path conformance/Cargo.toml -p conformance-wasm --target wasm32-unknown-unknown
//! CARGO_TARGET_DIR=target cargo test --offline --manifest-path conformance/Cargo.toml -p vfs-conformance -- --test-threads=1
//! ```
//! The suite requires its WASM artifact and fails if it is missing; no adapter
//! is silently skipped. CONFORMANCE_WASM can select a different artifact.
