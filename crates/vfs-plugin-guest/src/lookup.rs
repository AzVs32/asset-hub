use crate::api::{ErrorContext, Result};
use ah_vfs_storage::{
    ContractViolation, Entry, EntryType, Failure, IntermediateSymlinks, LookupStart, Operation,
    PathComponent, PathLookup, Rejection, StoragePath, SymlinkAction,
};
use vfs_plugin_protocol as p;

/// Resolve metadata on a backend with direct instance-root addressing.
pub fn direct_stat(
    value: p::Stat,
    mut stat: impl FnMut(&StoragePath) -> Result<Entry>,
) -> Result<Entry> {
    let (request, base, links) =
        p::decode_stat(value).map_err(|_| p::Failure::new(p::FailureCode::InvalidRequest))?;
    let (entry, ()) = direct_lookup(request.lookup(), &base, links, Operation::Stat, |path| {
        Ok((stat(path)?, ()))
    })?;
    Ok(entry)
}

/// Resolve a typed lookup and return its metadata together with backend binding data.
/// Direct-address backends reject navigation requiring stable directory identities.
pub fn direct_lookup<T>(
    lookup: PathLookup<'_>,
    base: &StoragePath,
    links: IntermediateSymlinks,
    operation: Operation,
    mut select: impl FnMut(&StoragePath) -> Result<(Entry, T)>,
) -> Result<(Entry, T)> {
    let context = ErrorContext::storage(operation);
    let reject = |reason| context.failure(Failure::Rejected(reason));
    let unsupported = || reject(Rejection::Unsupported);
    if lookup.path().start() == LookupStart::BaseDirectory && !base.is_root() {
        return Err(unsupported());
    }
    let mut path = StoragePath::root();
    let components = lookup.path().components();
    if components
        .iter()
        .any(|part| !matches!(part, PathComponent::Name(_)))
    {
        return Err(unsupported());
    }
    let mut checked = |path: &StoragePath| {
        let result = select(path)?;
        if result.0.path() != path {
            return Err(context.failure(Failure::Contract(ContractViolation::InvalidEntry)));
        }
        Ok(result)
    };
    for (index, component) in components.iter().enumerate() {
        let PathComponent::Name(name) = component else {
            return Err(unsupported());
        };
        path = path
            .join(name)
            .map_err(|_| reject(Rejection::PathTooLong))?;
        if index + 1 < components.len() {
            match checked(&path)?.0.entry_type() {
                EntryType::Directory => {}
                EntryType::Symlink if links == IntermediateSymlinks::Reject => {
                    return Err(reject(Rejection::SymlinkNotAllowed));
                }
                EntryType::Symlink => return Err(unsupported()),
                _ => return Err(reject(Rejection::NotDirectory)),
            }
        }
    }
    let result = checked(&path)?;
    if result.0.entry_type() == EntryType::Symlink
        && lookup.final_symlink() == SymlinkAction::FollowWithinInstance
    {
        return Err(unsupported());
    }
    lookup
        .check_existing_target(&path, result.0.entry_type())
        .map_err(reject)?;
    Ok(result)
}
