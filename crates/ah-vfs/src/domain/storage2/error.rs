//! Operational context and private diagnostics. Value validation stays in its own errors.
use super::{Failure, MutationOutcome, Operation};
use bon::bon;
use getset::CopyGetters;
use std::{error::Error, fmt};
use thiserror::Error;

/// A failed operation, with independent cause and mutation-outcome classifications.
///
/// There are deliberately no `From` conversions from path, attribute, range or
/// page validation errors. Request decoding returns those input errors; response
/// decoding explicitly chooses `Failure::Contract` and attaches the same cause.
/// Database corruption and driver configuration do not belong to this type.
///
/// `Display` and `Debug` expose only fixed public classifications. Diagnostics are
/// available solely through `Error::source`; adapters must not serialize them.
#[derive(Error, CopyGetters)]
#[error("{operation}: {failure}; {outcome}")]
#[getset(get_copy = "pub")]
pub struct StorageError {
    operation: Operation,
    failure: Failure,
    outcome: MutationOutcome,
    #[source]
    #[getset(skip)]
    source: Option<Box<dyn Error + Send + Sync>>,
}
#[bon]
impl StorageError {
    /// The adapter attests that the operation made no content/attribute changes.
    /// Use for failures before dispatch or a backend's confirmed atomic rejection;
    /// never infer this property solely from an error reason or transport status.
    #[builder(finish_fn = build, state_mod(vis = "pub"))]
    pub fn without_effects(
        operation: Operation,
        failure: Failure,
        #[builder(with = |source: impl Error + Send + Sync + 'static| Box::new(source) as Box<dyn Error + Send + Sync>)]
        source: Option<Box<dyn Error + Send + Sync>>,
    ) -> Self {
        Self {
            operation,
            failure,
            outcome: MutationOutcome::Unchanged,
            source,
        }
    }
    /// The request was dispatched and completion cannot be established.
    /// Mutations conservatively require reconciliation; read-only operations
    /// remain effect-free even when their result could not be decoded.
    #[builder(finish_fn = build, state_mod(vis = "pub"))]
    pub fn after_dispatch(
        operation: Operation,
        failure: Failure,
        #[builder(with = |source: impl Error + Send + Sync + 'static| Box::new(source) as Box<dyn Error + Send + Sync>)]
        source: Option<Box<dyn Error + Send + Sync>>,
    ) -> Self {
        Self {
            operation,
            failure,
            outcome: if operation.may_mutate() {
                MutationOutcome::Unknown
            } else {
                MutationOutcome::Unchanged
            },
            source,
        }
    }
}
impl fmt::Debug for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageError")
            .field("operation", &self.operation)
            .field("failure", &self.failure)
            .field("outcome", &self.outcome)
            .field("has_source", &self.source.is_some())
            .finish()
    }
}
