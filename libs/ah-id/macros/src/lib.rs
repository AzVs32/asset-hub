//! Derive implementation for the `ah-id` crate.
//!
//! Applications should use the `ah_id::Id` re-export, alongside the value types
//! and `Generate` trait provided by `ah-id`.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod expand;

/// Derive selected ID capabilities for a tuple struct with one private field.
///
/// Select capabilities with `#[id(...)]`: `generate`, `display`, `parse`,
/// `as_ref` and `deref`. At least one option is required. Invalid struct shapes
/// or options produce compile-time diagnostics. The `ah_id::Id` documentation
/// describes each capability and its validation requirements.
#[proc_macro_derive(Id, attributes(id))]
pub fn derive_id(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
