mod args;
mod expand;
mod paths;
mod serde;

use proc_macro::TokenStream;

/// Implements `ConfigSection` for a struct, using `key` and an optional `validate` function.
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
