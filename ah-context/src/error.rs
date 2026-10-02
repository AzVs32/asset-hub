use std::error::Error;

/// Errors from registering, constructing, or accessing context values.
///
/// Initialization failures display the type's context without repeating the
/// underlying reason. Use [`Error::source`] or an error-chain reporter for the
/// original initializer error and its causes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ContextError {
    #[error("call ah_context::load() or ah_context::load_with() before accessing the context")]
    NotInitialized,
    /// A custom builder was submitted after the global context was published.
    #[error("the global context is already initialized")]
    AlreadyInitialized,
    #[error("context type `{type_name}` is not registered")]
    NotRegistered { type_name: &'static str },
    #[error("context type `{type_name}` is registered more than once")]
    DuplicateRegistration { type_name: &'static str },
    /// An initializer failed; the original error is available through `source()`.
    #[error("failed to initialize context type `{type_name}`")]
    Initialization {
        type_name: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}
