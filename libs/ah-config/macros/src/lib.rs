//! Derive implementation for `ah-config`.
//!
//! Applications should depend on `ah-config` and use its `Config` and
//! `ConfigValue` re-exports. Both generate Serde `Serialize` and `Deserialize`
//! implementations using the runtime crate's Serde dependency.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod expand;

/// Register a configuration section and generate Serde `Serialize` and `Deserialize`.
///
/// Requires a concrete struct or enum without generic parameters or a `where`
/// clause. Registered values must be owned, `Send + Sync + 'static`; their fields
/// must support the generated serialization and deserialization implementations.
/// Unions are unsupported. Do not also derive or implement Serde's traits for
/// the same type. Other standard derives such as `Clone` and `Debug` are optional.
///
/// | Attribute location | Supported `#[config(...)]` options |
/// | --- | --- |
/// | Type | `key` (required), `parent`, `spaces`, `default`, `validate`, `tag` (enums only), `deny_unknown_fields` |
/// | Struct or variant field | `default`, `rename` (named fields) |
/// | Enum variant | `rename` |
///
/// `key = "vfs"` selects a root TOML section. A child declares
/// `parent = VfsConfig, key = "wasm"` to select `vfs.wasm` without adding a field
/// to `VfsConfig`. Keys must be single nonempty ASCII segments containing letters,
/// digits, underscores or hyphens; dotted keys are rejected. Parent types may
/// come from another crate. They must be named structs derived with `Config`
/// without `deny_unknown_fields`. The child key cannot match an external field
/// name of the parent. These restrictions are checked during compilation.
/// Parents and children are retrieved independently with `LoadedConfig::get`.
/// Use `ConfigValue` for a child owned as a field instead of registering it again.
/// This derive implements the public `ConfigSection` trait, including its full
/// `KEY` path. Typed reads and generic read helpers require that trait.
///
/// `spaces = ["default", "plugins"]` replaces the implicit membership in
/// `default`; it does not prefix the key. Parent relationships do not inherit
/// space membership or automatically select the parent for loading.
/// Fields and variants always use snake_case; `rename = "name"` overrides one
/// external name. Explicit keys and tags are used unchanged. `rename_all` and
/// `#[serde(...)]` attributes are rejected.
///
/// A field's `default` uses its type's `Default`; `default = expression` evaluates
/// a value or function call such as `default = factory()`. Missing `Option<T>`
/// fields become `None`; other fields without defaults remain required.
///
/// Every type-level `default` declaration generates `Default`, for both this
/// macro and [`derive@ConfigValue`]. On a struct, bare `default` constructs the
/// value from field defaults. `default = expression` returns that expression,
/// including function calls. On an enum, `default = Self::Variant` constructs
/// that variant from its field defaults. Automatic field construction requires
/// explicit field defaults or `Option<T>`.
///
/// This macro uses the generated `Default` as the section's loading defaults.
/// Calling `Default::default()` constructs a value without running validation.
/// Factories must not call the generated `Default` recursively. Do not also
/// implement or derive `Default` when a type-level default is declared. Without
/// that declaration, an existing `Default` implementation is allowed but is not
/// implicitly used for loading; field defaults continue to apply independently.
///
/// Enums with payloads require `tag = "backend"` and use Serde's internally
/// tagged representation. Newtype payloads must serialize as maps; variants
/// with multiple unnamed fields are unsupported.
/// Unit-only enums without a tag use string values. Default variants are selected
/// on the enum type, not by placing `default` on a variant.
///
/// `validate = check` accepts a function path with the signature
/// `fn(&T) -> Result<(), E>`, where `E` converts to `Box<dyn Error + Send + Sync>`.
/// It runs after conversion and default merging for selected sections, including
/// their nested values. A failure prevents the snapshot from being returned.
///
/// A strict parent cannot be extended, even from another crate:
///
/// ```compile_fail
/// #[derive(ah_config::Config)]
/// #[config(key = "vfs", deny_unknown_fields)]
/// struct VfsConfig {}
///
/// #[derive(ah_config::Config)]
/// #[config(parent = VfsConfig, key = "wasm")]
/// struct WasmConfig { memory_limit_mb: u64 }
/// ```
#[proc_macro_derive(Config, attributes(config))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    expand::derive(parse_macro_input!(input as DeriveInput), true)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Generate Serde `Serialize` and `Deserialize` for a nested configuration value.
///
/// Uses the same concrete-type, serialization, naming and enum representation
/// restrictions as [`derive@Config`], but does not register a key or a space.
/// Retrieve nested values through their owning section. This derive does not
/// implement `ConfigSection`, so passing the type to `LoadedConfig::get` is a
/// compile-time error. Standard derives such as `Clone` and `Debug` remain optional.
///
/// | Attribute location | Supported `#[config(...)]` options |
/// | --- | --- |
/// | Type | `default`, `tag` (enums only), `deny_unknown_fields` |
/// | Struct or variant field | `default`, `rename` (named fields) |
/// | Enum variant | `rename` |
///
/// Type-level defaults follow the same rules as [`derive@Config`]: bare `default`
/// (structs only) or `default = expression` generates `Default`. Selecting
/// `default = Self::Variant` constructs that variant from its field defaults.
/// Without a type-level default, an existing manual or derived `Default` is
/// allowed. An owning field must opt into using either implementation with
/// `#[config(default)]` if it should accept an absent value. A partially supplied
/// nested struct uses its field defaults, rather than implicitly invoking its
/// type's `Default`; inherited parent defaults still merge recursively.
/// Calling `Default::default()` does not run configuration validation.
///
/// `key`, `parent`, `spaces` and `validate` are unsupported here. Validate nested values in
/// the owning `Config` section's validator. Do not also implement or derive
/// Serde's traits, or `Default` when it is generated by this macro.
#[proc_macro_derive(ConfigValue, attributes(config))]
pub fn derive_config_value(input: TokenStream) -> TokenStream {
    expand::derive(parse_macro_input!(input as DeriveInput), false)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
