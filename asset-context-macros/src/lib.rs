mod args;
mod cfg;
mod expand;
mod paths;

use proc_macro::TokenStream;

/// Automatically registers a type in the global context.
///
/// Registration is always automatic. Without `init`, the type must implement
/// `Default`. `init = path` accepts a function returning `Result<Self, E>`,
/// where `E: Into<Box<dyn std::error::Error + Send + Sync>>`.
/// All values must be `Send + Sync + 'static`.
/// The struct must be non-generic so its concrete type can be registered.
#[proc_macro_attribute]
pub fn context(args: TokenStream, input: TokenStream) -> TokenStream {
    match expand::context(args, input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Registers a concrete type without annotating or modifying its definition.
///
/// `register!(Type)` uses `Default`; `register!(Type, init = path)` uses the same
/// fallible construction contract as `#[context(init = path)]`.
#[proc_macro]
pub fn register(input: TokenStream) -> TokenStream {
    match expand::register(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
