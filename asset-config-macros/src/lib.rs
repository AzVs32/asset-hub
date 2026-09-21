mod args;
mod auto;
mod expand;
mod paths;
mod serde;

use proc_macro::TokenStream;

/// Implements `ConfigSection` for a struct and registers it for automatic loading.
///
/// At registration, `key` must be a case-sensitive, dot-separated path of
/// ASCII letters, digits, `_`, and `-`. Registered keys cannot overlap as
/// parent and child paths.
/// `validate` names an optional validation function. Set
/// `auto = false` to exclude this section from automatic loading and register
/// a concrete type with `Registry` instead.
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
