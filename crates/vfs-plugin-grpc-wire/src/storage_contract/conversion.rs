use super::*;
use ah_vfs_storage as d;
use prost::Message;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NANOS: i128 = 1_000_000_000;

fn timestamp(time: SystemTime) -> Result<Timestamp, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidEntry;
    let signed = |duration: Duration| {
        i128::from(duration.as_secs()) * NANOS + i128::from(duration.subsec_nanos())
    };
    let total = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => signed(duration),
        Err(error) => -signed(error.duration()),
    };
    Ok(Timestamp {
        seconds: i64::try_from(total.div_euclid(NANOS)).map_err(|_| invalid)?,
        nanos: u32::try_from(total.rem_euclid(NANOS)).map_err(|_| invalid)?,
    })
}

fn parse_timestamp(wire: Timestamp) -> Result<SystemTime, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidEntry;
    if i128::from(wire.nanos) >= NANOS {
        return Err(invalid);
    }
    let total = i128::from(wire.seconds) * NANOS + i128::from(wire.nanos);
    let duration = Duration::new(
        u64::try_from(total.abs() / NANOS).map_err(|_| invalid)?,
        u32::try_from(total.abs() % NANOS).map_err(|_| invalid)?,
    );
    let time = if total < 0 {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    }
    .ok_or(invalid)?;
    if timestamp(time)? != wire {
        return Err(invalid);
    }
    Ok(time)
}

pub fn encode_entry(entry: d::Entry) -> Result<Entry, d::ContractViolation> {
    let (kind, size) = match entry.entry_type() {
        d::EntryType::File { size, .. } => (EntryKind::File, size),
        d::EntryType::Directory => (EntryKind::Directory, None),
        d::EntryType::Symlink => (EntryKind::Symlink, None),
        _ => return Err(d::ContractViolation::InvalidEntry),
    };
    Ok(Entry {
        address: entry.path().as_str().into(),
        kind: kind.into(),
        size,
        modified: entry.modified().map(timestamp).transpose()?,
    })
}

pub fn decode_entry(entry: Entry) -> Result<d::Entry, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidEntry;
    let address = d::StoragePath::parse(&entry.address).map_err(|_| invalid)?;
    let kind = match (EntryKind::try_from(entry.kind), entry.size) {
        (Ok(EntryKind::File), size) => d::EntryType::file(size),
        (Ok(EntryKind::Directory), None) => d::EntryType::Directory,
        (Ok(EntryKind::Symlink), None) => d::EntryType::Symlink,
        _ => return Err(invalid),
    };
    d::Entry::new(
        address,
        kind,
        entry.modified.map(parse_timestamp).transpose()?,
    )
    .map_err(|_| invalid)
}

pub fn encode_selection(selection: &d::DirectorySelection) -> Selection {
    Selection {
        address: selection.address().as_str().into(),
        identity: selection.identity().as_bytes().to_vec(),
    }
}

fn parse_selection(
    selection: Selection,
) -> Result<(d::StoragePath, d::DirectoryIdentity), d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidPage;
    let address = d::StoragePath::parse(&selection.address).map_err(|_| invalid)?;
    let identity = selection
        .identity
        .as_slice()
        .try_into()
        .map_err(|_| invalid)?;
    Ok((address, d::DirectoryIdentity::from_bytes(identity)))
}

/// Decode a separately correlated selection from the authenticated executor.
pub fn decode_selection(
    selection: Selection,
) -> Result<d::DirectorySelection, d::ContractViolation> {
    let (address, identity) = parse_selection(selection)?;
    Ok(d::DirectorySelection::from_lookup(address, identity))
}

pub fn encode_page(page: d::DirectoryPage) -> Result<Page, d::ContractViolation> {
    let (selection, entries, next) = page.into_parts();
    let invalid = d::ContractViolation::InvalidPage;
    if entries.len() > MAX_PAGE_ENTRIES {
        return Err(invalid);
    }
    let completion = match next {
        Some(name) => page::Completion::After(name.as_str().into()),
        None => page::Completion::End(Unit {}),
    };
    let mut encoded = Vec::new();
    for entry in entries {
        encode_entry(entry)?
            .encode_length_delimited(&mut encoded)
            .map_err(|_| invalid)?;
    }
    Ok(Page {
        reported: Some(encode_selection(&selection)),
        entries: encoded,
        completion: Some(completion),
    })
}

pub fn decode_page(
    bound: &d::BoundListRequest<'_>,
    page: Page,
) -> Result<d::DirectoryPage, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidPage;
    if page.entries.len() > MAX_FRAME_BYTES {
        return Err(invalid);
    }
    let (reported, identity) = parse_selection(page.reported.ok_or(invalid)?)?;
    let after = match page.completion.ok_or(invalid)? {
        page::Completion::End(_) => None,
        page::Completion::After(name) => Some(d::EntryName::parse(&name).map_err(|_| invalid)?),
    };
    let mut bytes = page.entries.as_slice();
    let mut entries = Vec::new();
    while !bytes.is_empty() {
        if entries.len() == MAX_PAGE_ENTRIES.min(bound.size().get()) {
            return Err(invalid);
        }
        let entry = Entry::decode_length_delimited(&mut bytes).map_err(|_| invalid)?;
        entries.push(decode_entry(entry)?);
    }
    d::DirectoryPage::from_parts(bound, &reported, identity, entries, after).map_err(|_| invalid)
}

fn operation(op: d::Operation) -> Option<Operation> {
    Some(match op {
        d::Operation::Stat => Operation::Stat,
        d::Operation::List => Operation::List,
        d::Operation::ReadLink => Operation::ReadLink,
        d::Operation::Read => Operation::Read,
        d::Operation::Write => Operation::Write,
        d::Operation::CreateDirectory => Operation::CreateDirectory,
        d::Operation::Remove => Operation::Remove,
        d::Operation::GetPermissions => Operation::GetPermissions,
        _ => return None,
    })
}

// A single table defines both directions and deliberately rejects unknown tags.
const CAUSES: &[(&str, &str, d::Failure)] = {
    use d::{BackendFailure as B, ContractViolation as C, Failure::*, Rejection as R};
    &[
        ("rejected", "not-found", Rejected(R::NotFound)),
        ("rejected", "already-exists", Rejected(R::AlreadyExists)),
        ("rejected", "not-directory", Rejected(R::NotDirectory)),
        ("rejected", "is-directory", Rejected(R::IsDirectory)),
        ("rejected", "not-regular-file", Rejected(R::NotRegularFile)),
        ("rejected", "not-symlink", Rejected(R::NotSymlink)),
        ("rejected", "root-protected", Rejected(R::RootProtected)),
        (
            "rejected",
            "symlink-not-allowed",
            Rejected(R::SymlinkNotAllowed),
        ),
        (
            "rejected",
            "too-many-symlinks",
            Rejected(R::TooManySymlinks),
        ),
        ("rejected", "path-too-long", Rejected(R::PathTooLong)),
        (
            "rejected",
            "directory-not-empty",
            Rejected(R::DirectoryNotEmpty),
        ),
        ("rejected", "unsupported", Rejected(R::Unsupported)),
        ("rejected", "conflict", Rejected(R::Conflict)),
        ("rejected", "limit-exceeded", Rejected(R::LimitExceeded)),
        ("backend", "capacity-exceeded", Backend(B::CapacityExceeded)),
        (
            "backend",
            "unrepresentable-link-target",
            Backend(B::UnrepresentableLinkTarget),
        ),
        ("backend", "stale", Backend(B::Stale)),
        ("backend", "closed", Backend(B::Closed)),
        ("backend", "other", Backend(B::Other)),
        ("backend", "unavailable", Backend(B::Unavailable)),
        ("backend", "timed-out", Backend(B::TimedOut)),
        ("backend", "cancelled", Backend(B::Cancelled)),
        ("contract", "invalid-entry", Contract(C::InvalidEntry)),
        ("contract", "invalid-page", Contract(C::InvalidPage)),
        ("contract", "invalid-response", Contract(C::InvalidResponse)),
    ]
};

pub fn encode_failure(error: &d::StorageError) -> Result<Failure, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidResponse;
    let op = operation(error.operation()).ok_or(invalid)?;
    let (family, reason, _) = CAUSES
        .iter()
        .find(|(_, _, cause)| *cause == error.failure())
        .ok_or(invalid)?;
    let outcome = match error.outcome() {
        d::MutationOutcome::Unchanged => Outcome::Unchanged,
        d::MutationOutcome::Unknown => Outcome::Unknown,
    };
    Ok(Failure {
        operation: op.into(),
        family: (*family).into(),
        reason: (*reason).into(),
        outcome: outcome.into(),
    })
}

pub fn decode_failure(
    expected: d::Operation,
    failure: Failure,
) -> Result<d::StorageError, d::ContractViolation> {
    let invalid = d::ContractViolation::InvalidResponse;
    if operation(expected).map(i32::from) != Some(failure.operation) {
        return Err(invalid);
    }
    let (_, _, cause) = CAUSES
        .iter()
        .find(|(family, reason, _)| *family == failure.family && *reason == failure.reason)
        .ok_or(invalid)?;
    match Outcome::try_from(failure.outcome) {
        Ok(Outcome::Unchanged) => Ok(d::StorageError::without_effects(expected, *cause, None)),
        Ok(Outcome::Unknown) if expected.may_mutate() => {
            Ok(d::StorageError::after_dispatch(expected, *cause, None))
        }
        _ => Err(invalid),
    }
}
