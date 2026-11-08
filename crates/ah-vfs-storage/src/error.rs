//! Operational context and private diagnostics. Value validation stays in its own errors.
use super::{Failure, MutationOutcome, Operation};
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
impl StorageError {
    /// Attest [`MutationOutcome::Unchanged`] for the entire failed attempt.
    /// Evidence can be a local rejection before dispatching any step that can
    /// change covered state, confinement to private staging with publication
    /// ruled out, or a correlated backend guarantee that the attempt neither
    /// applied nor can apply later.
    /// Include earlier steps and visible parent changes in this check.
    /// A failure reason, transport status or later matching read is not proof.
    pub fn without_effects(
        operation: Operation,
        failure: Failure,
        source: Option<Box<dyn Error + Send + Sync>>,
    ) -> Self {
        Self {
            operation,
            failure,
            outcome: MutationOutcome::Unchanged,
            source,
        }
    }
    /// Use when mutation dispatch occurred or cannot be ruled out, and there
    /// is no proof of [`MutationOutcome::Unchanged`] for the whole attempt.
    /// Also use for known effects, including partial creation or publication.
    /// Read-only operations are `Unchanged` under their execution contract.
    /// A successful abort or cleanup cannot resolve an earlier uncertain commit.
    pub fn after_dispatch(
        operation: Operation,
        failure: Failure,
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
            .finish()
    }
}
