use crate::api::{ErrorContext, Result};
use ah_vfs_storage::{Failure, Operation, Rejection};
use vfs_plugin_protocol as p;

/// Decode metadata operands for a backend with only direct instance-root addressing.
/// Operations requiring bound working directories or navigation must be rejected.
pub fn direct_stat(
    value: p::Stat,
    mut stat: impl FnMut(&ah_vfs_storage::StoragePath) -> Result<ah_vfs_storage::Entry>,
) -> Result<ah_vfs_storage::Entry> {
    use ah_vfs_storage::{LookupStart, PathComponent, StoragePath};
    let (request, base, links) =
        p::decode_stat(value).map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
    let context = ErrorContext::storage(Operation::Stat);
    let unsupported = || context.failure(Failure::Rejected(Rejection::Unsupported));
    if request.path().start() == LookupStart::BaseDirectory && !base.is_root() {
        return Err(unsupported());
    }
    let mut path = StoragePath::root();
    let components = request.path().components();
    if components
        .iter()
        .any(|part| !matches!(part, PathComponent::Name(_)))
    {
        return Err(unsupported());
    }
    for (index, component) in components.iter().enumerate() {
        let PathComponent::Name(name) = component else {
            return Err(unsupported());
        };
        path = path
            .join(name)
            .map_err(|_| context.failure(Failure::Rejected(Rejection::PathTooLong)))?;
        if index + 1 < components.len() {
            let parent = stat(&path)?;
            if parent.path() != &path {
                return Err(context.failure(Failure::Contract(
                    ah_vfs_storage::ContractViolation::InvalidEntry,
                )));
            }
            if parent.entry_type() == ah_vfs_storage::EntryType::Symlink {
                let reason = if links == ah_vfs_storage::IntermediateSymlinks::Reject {
                    Rejection::SymlinkNotAllowed
                } else {
                    Rejection::Unsupported
                };
                return Err(context.failure(Failure::Rejected(reason)));
            }
            if parent.entry_type() != ah_vfs_storage::EntryType::Directory {
                return Err(context.failure(Failure::Rejected(Rejection::NotDirectory)));
            }
        }
    }
    let entry = stat(&path)?;
    if entry.path() != &path {
        return Err(context.failure(Failure::Contract(
            ah_vfs_storage::ContractViolation::InvalidEntry,
        )));
    }
    if entry.entry_type() == ah_vfs_storage::EntryType::Symlink
        && request.lookup().final_symlink() == ah_vfs_storage::SymlinkAction::FollowWithinInstance
    {
        return Err(unsupported());
    }
    request
        .lookup()
        .check_existing_target(&path, entry.entry_type())
        .map_err(|cause| context.failure(Failure::Rejected(cause)))?;
    Ok(entry)
}
