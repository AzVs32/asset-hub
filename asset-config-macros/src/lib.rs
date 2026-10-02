mod args;
mod auto;
mod cfg;
mod expand;
mod paths;
mod serde;

use proc_macro::TokenStream;

/// Implements `ConfigSection` for a struct and registers it for automatic loading.
///
/// At registration, `key` must be a case-sensitive, dot-separated path of
/// ASCII letters, digits, `_`, and `-`. Registered keys cannot overlap as
/// parent and child paths.
/// `validate` names an optional validation function returning `Result<(), E>`,
/// where `E: Into<Box<dyn std::error::Error + Send + Sync>>`. Set
/// `auto = false` to use a non-generic section only in selected registries.
/// Generic definitions generate the trait implementation without registering a
/// type; use `register!(Generic<Concrete>)` to select an automatic instance.
/// Explicit `auto = true` on a generic definition is a compile-time error.
///
/// Conditional-compilation attributes (`cfg` and nested `cfg_attr`) also gate
/// the generated implementation and automatic registration.
///
/// Place this attribute before any `#[derive(...)]` on the struct so existing Serde derives
/// are visible and are not generated a second time.
#[proc_macro_attribute]
pub fn config(args: TokenStream, input: TokenStream) -> TokenStream {
    match expand::config(args, input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Registers a concrete `ConfigSection`, including a generic instance.
///
/// The type's existing `ConfigSection::KEY` is used. This macro does not modify
/// the definition or implement the trait. Place it at module scope.
#[proc_macro]
pub fn register(input: TokenStream) -> TokenStream {
    match expand::register(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
