use std::error::Error;

/// Errors from loading or accessing the global context.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("call ah_context::load() before accessing the context")]
    NotInitialized,
    #[error("context type `{type_name}` is not registered")]
    NotRegistered { type_name: &'static str },
    #[error("context type `{type_name}` is registered more than once")]
    DuplicateRegistration { type_name: &'static str },
    #[error("failed to initialize context type `{type_name}`: {source}")]
    Initialization {
        type_name: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}
