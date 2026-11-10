use crate::api::{ErrorContext, Result};
use ah_vfs_storage::{
    ContractViolation, Entry, EntryType, Failure, IntermediateSymlinks, LookupStart, Operation,
    PathComponent, PathLookup, Rejection, StatRequest, StoragePath, SymlinkAction, WriteRequest,
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
    let mut components = lookup.path().components();
    if components == [PathComponent::CurrentDirectory] {
        components = &[];
    }
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

/// Resolve a write operand without interpreting absence as permission to replace.
/// The backend must apply the returned binding data as publication conditions.
pub fn direct_write_destination<T>(
    request: &WriteRequest,
    base: &StoragePath,
    links: IntermediateSymlinks,
    mut select: impl FnMut(&StoragePath) -> Result<(Entry, T)>,
) -> Result<(StoragePath, T, Option<T>)> {
    let op = Operation::Write;
    let context = ErrorContext::storage(op);
    let reject = |reason| context.failure(Failure::Rejected(reason));
    let Some((parent, name)) = request.path().split_final_name() else {
        direct_lookup(request.lookup(), base, links, op, select)?;
        return Err(reject(Rejection::NotDirectory));
    };
    let final_link = match links {
        IntermediateSymlinks::FollowWithinInstance => SymlinkAction::FollowWithinInstance,
        IntermediateSymlinks::Reject => SymlinkAction::Preserve,
    };
    let (parent, parent_binding) = direct_lookup(
        StatRequest::new(parent, final_link).lookup(),
        base,
        links,
        op,
        &mut select,
    )?;
    match parent.entry_type() {
        EntryType::Directory => {}
        EntryType::Symlink => return Err(reject(Rejection::SymlinkNotAllowed)),
        _ => return Err(reject(Rejection::NotDirectory)),
    }
    let path = parent
        .path()
        .join(name)
        .map_err(|_| reject(Rejection::PathTooLong))?;
    let destination = match select(&path) {
        Ok((entry, value)) if entry.path() == &path => {
            request
                .lookup()
                .check_existing_target(&path, entry.entry_type())
                .map_err(reject)?;
            Some(value)
        }
        Ok(_) => return Err(context.failure(Failure::Contract(ContractViolation::InvalidEntry))),
        Err(error)
            if error
                .storage_error()
                .is_some_and(|error| error.failure() == Failure::Rejected(Rejection::NotFound)) =>
        {
            request.check_new_destination(&path).map_err(reject)?;
            None
        }
        Err(error) => return Err(error),
    };
    Ok((path, parent_binding, destination))
}
