//! Per-attempt operation and effect evidence at the guest protocol boundary.
use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};
use std::{cell::Cell, rc::Rc};
use vfs_plugin_protocol as p;
#[derive(Clone)]
pub struct ErrorContext {
    operation: Option<Operation>,
    effects: Rc<Cell<bool>>,
}
impl ErrorContext {
    pub fn lifecycle() -> Self {
        Self {
            operation: None,
            effects: Rc::new(Cell::new(false)),
        }
    }
    pub fn storage(operation: Operation) -> Self {
        Self {
            operation: Some(operation),
            effects: Rc::new(Cell::new(false)),
        }
    }
    pub fn for_request(request: &p::Operation) -> Self {
        request
            .storage_operation()
            .map(Self::storage)
            .unwrap_or_else(Self::lifecycle)
    }
    /// Call only when covered state may be changed; private upload staging is excluded.
    pub fn dispatched(&self) {
        self.effects.set(true);
    }
    pub fn failure(&self, cause: Failure) -> crate::api::Failure {
        if let Some(operation) = self.operation {
            let error = if self.effects.get() {
                StorageError::after_dispatch(operation, cause, None)
            } else {
                StorageError::without_effects(operation, cause, None)
            };
            return error.into();
        }
        let code = match cause {
            Failure::Rejected(Rejection::AccessDenied) => p::FailureCode::LifecycleAccessDenied,
            Failure::Rejected(Rejection::Unsupported) => p::FailureCode::LifecycleUnsupported,
            Failure::Rejected(Rejection::Conflict) => p::FailureCode::LifecycleConflict,
            Failure::Rejected(Rejection::LimitExceeded)
            | Failure::Backend(BackendFailure::CapacityExceeded) => {
                p::FailureCode::LifecycleCapacityExceeded
            }
            Failure::Backend(BackendFailure::Unavailable) => p::FailureCode::LifecycleUnavailable,
            Failure::Backend(BackendFailure::TimedOut) => p::FailureCode::LifecycleTimedOut,
            Failure::Backend(BackendFailure::Closed | BackendFailure::Stale) => {
                p::FailureCode::LifecycleStale
            }
            Failure::Backend(BackendFailure::Cancelled) => p::FailureCode::LifecycleCancelled,
            Failure::Contract(_) => p::FailureCode::LifecycleInvalidDeclaration,
            _ => p::FailureCode::LifecycleBackend,
        };
        p::Failure::new(code).into()
    }
}
