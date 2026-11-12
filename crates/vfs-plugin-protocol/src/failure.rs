//! Lossless storage failure messages, independent of lifecycle status codes.
use crate::{MutationOutcome, StorageFailure};
use ah_vfs_storage as d;
const OPERATIONS: &[(&str, d::Operation)] = &[
    ("stat", d::Operation::Stat),
    ("list", d::Operation::List),
    ("read", d::Operation::Read),
    ("read-link", d::Operation::ReadLink),
    ("write", d::Operation::Write),
    ("create-directory", d::Operation::CreateDirectory),
    ("remove", d::Operation::Remove),
    ("rename", d::Operation::Rename),
    ("get-permissions", d::Operation::GetPermissions),
    ("set-permissions", d::Operation::SetPermissions),
    ("list-xattrs", d::Operation::ListXattrs),
    ("get-xattr", d::Operation::GetXattr),
    ("set-xattr", d::Operation::SetXattr),
    ("remove-xattr", d::Operation::RemoveXattr),
];
/// Stable names for the operations that use a bound lookup.
pub fn encode_lookup_purpose(operation: d::Operation) -> Option<&'static str> {
    OPERATIONS
        .iter()
        .find(|(_, op)| *op == operation)
        .map(|(name, _)| *name)
}

pub fn decode_lookup_purpose(name: &str) -> Option<d::Operation> {
    OPERATIONS
        .iter()
        .find_map(|(tag, op)| (*tag == name).then_some(*op))
}

const CAUSES: &[(&str, &str, d::Failure)] = &[
    ("caller", "invalid-binding", d::Failure::InvalidBinding),
    ("caller", "invalid-request", d::Failure::InvalidRequest),
    (
        "rejected",
        "not-found",
        d::Failure::Rejected(d::Rejection::NotFound),
    ),
    (
        "rejected",
        "already-exists",
        d::Failure::Rejected(d::Rejection::AlreadyExists),
    ),
    (
        "rejected",
        "access-denied",
        d::Failure::Rejected(d::Rejection::AccessDenied),
    ),
    (
        "rejected",
        "not-directory",
        d::Failure::Rejected(d::Rejection::NotDirectory),
    ),
    (
        "rejected",
        "is-directory",
        d::Failure::Rejected(d::Rejection::IsDirectory),
    ),
    (
        "rejected",
        "not-regular-file",
        d::Failure::Rejected(d::Rejection::NotRegularFile),
    ),
    (
        "rejected",
        "not-symlink",
        d::Failure::Rejected(d::Rejection::NotSymlink),
    ),
    (
        "rejected",
        "name-too-long",
        d::Failure::Rejected(d::Rejection::NameTooLong),
    ),
    (
        "rejected",
        "unsupported-name",
        d::Failure::Rejected(d::Rejection::UnsupportedName),
    ),
    (
        "rejected",
        "path-too-long",
        d::Failure::Rejected(d::Rejection::PathTooLong),
    ),
    (
        "rejected",
        "directory-not-empty",
        d::Failure::Rejected(d::Rejection::DirectoryNotEmpty),
    ),
    (
        "rejected",
        "unsupported",
        d::Failure::Rejected(d::Rejection::Unsupported),
    ),
    (
        "rejected",
        "conflict",
        d::Failure::Rejected(d::Rejection::Conflict),
    ),
    (
        "rejected",
        "cross-device",
        d::Failure::Rejected(d::Rejection::CrossDevice),
    ),
    (
        "rejected",
        "attribute-not-found",
        d::Failure::Rejected(d::Rejection::AttributeNotFound),
    ),
    (
        "rejected",
        "limit-exceeded",
        d::Failure::Rejected(d::Rejection::LimitExceeded),
    ),
    (
        "backend",
        "unavailable",
        d::Failure::Backend(d::BackendFailure::Unavailable),
    ),
    (
        "backend",
        "timed-out",
        d::Failure::Backend(d::BackendFailure::TimedOut),
    ),
    (
        "backend",
        "cancelled",
        d::Failure::Backend(d::BackendFailure::Cancelled),
    ),
    (
        "backend",
        "rate-limited",
        d::Failure::Backend(d::BackendFailure::RateLimited),
    ),
    (
        "backend",
        "capacity-exceeded",
        d::Failure::Backend(d::BackendFailure::CapacityExceeded),
    ),
    (
        "backend",
        "corrupt-data",
        d::Failure::Backend(d::BackendFailure::CorruptData),
    ),
    (
        "backend",
        "unrepresentable-name",
        d::Failure::Backend(d::BackendFailure::UnrepresentableName),
    ),
    (
        "backend",
        "unrepresentable-link-target",
        d::Failure::Backend(d::BackendFailure::UnrepresentableLinkTarget),
    ),
    (
        "backend",
        "closed",
        d::Failure::Backend(d::BackendFailure::Closed),
    ),
    (
        "backend",
        "stale",
        d::Failure::Backend(d::BackendFailure::Stale),
    ),
    (
        "backend",
        "other",
        d::Failure::Backend(d::BackendFailure::Other),
    ),
    (
        "contract",
        "invalid-declaration",
        d::Failure::Contract(d::ContractViolation::InvalidDeclaration),
    ),
    (
        "contract",
        "invalid-response",
        d::Failure::Contract(d::ContractViolation::InvalidResponse),
    ),
    (
        "contract",
        "invalid-entry",
        d::Failure::Contract(d::ContractViolation::InvalidEntry),
    ),
    (
        "contract",
        "invalid-page",
        d::Failure::Contract(d::ContractViolation::InvalidPage),
    ),
    (
        "contract",
        "invalid-attributes",
        d::Failure::Contract(d::ContractViolation::InvalidAttributes),
    ),
    (
        "contract",
        "invalid-stream",
        d::Failure::Contract(d::ContractViolation::InvalidStream),
    ),
    (
        "contract",
        "capability-mismatch",
        d::Failure::Contract(d::ContractViolation::CapabilityMismatch),
    ),
];
pub fn encode_storage_error(
    error: &d::StorageError,
) -> Result<StorageFailure, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidResponse;
    let operation = OPERATIONS
        .iter()
        .find(|(_, op)| *op == error.operation())
        .ok_or(invalid)?
        .0;
    let (family, reason, _) = CAUSES
        .iter()
        .find(|(_, _, failure)| *failure == error.failure())
        .ok_or(invalid)?;
    Ok(StorageFailure {
        operation: operation.into(),
        family: (*family).into(),
        reason: (*reason).into(),
        outcome: match error.outcome() {
            d::MutationOutcome::Unchanged => MutationOutcome::Unchanged,
            d::MutationOutcome::Unknown => MutationOutcome::Unknown,
        }
        .into(),
    })
}
pub fn decode_storage_error(
    error: StorageFailure,
) -> Result<d::StorageError, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidResponse;
    let operation = OPERATIONS
        .iter()
        .find(|(name, _)| *name == error.operation)
        .ok_or(invalid)?
        .1;
    let failure = CAUSES
        .iter()
        .find(|(family, reason, _)| *family == error.family && *reason == error.reason)
        .ok_or(invalid)?
        .2;
    match MutationOutcome::try_from(error.outcome) {
        Ok(MutationOutcome::Unchanged) => {
            Ok(d::StorageError::without_effects(operation, failure, None))
        }
        Ok(MutationOutcome::Unknown) if operation.may_mutate() => {
            Ok(d::StorageError::after_dispatch(operation, failure, None))
        }
        _ => Err(invalid),
    }
}
