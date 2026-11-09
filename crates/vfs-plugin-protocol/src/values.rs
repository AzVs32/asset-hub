//! Shared conversions for plugin metadata and declarations.
use crate as p;
use ah_vfs_storage::{
    AttributeSupport, Capabilities, ContractViolation, DirectorySupport, Entry, EntryError,
    EntryType, PathError, Publication, ReadSupport, RenameMode, RenameSupport, StoragePath,
    WriteMode, WriteModes, WriteSupport,
};
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn encode_capabilities(c: Capabilities) -> Vec<i32> {
    let mut names = Vec::new();
    let write = c.write();
    for (enabled, name) in [
        (c.stat(), p::Capability::Stat),
        (c.list(), p::Capability::List),
        (c.read_link(), p::Capability::ReadLink),
        (c.directories().can_create(), p::Capability::CreateDir),
        (c.remove(), p::Capability::Remove),
        (c.directories().persistent_empty(), p::Capability::EmptyDirs),
        (
            write.supports(WriteMode::CreateNew, Publication::Staged),
            p::Capability::WriteNew,
        ),
        (
            write.supports(WriteMode::Replace, Publication::Staged),
            p::Capability::WriteReplace,
        ),
        (
            matches!(
                write,
                WriteSupport::Staged {
                    atomic_publish: true,
                    ..
                }
            ),
            p::Capability::AtomicPublish,
        ),
        (
            c.rename().supports(RenameMode::NoReplace),
            p::Capability::RenameNew,
        ),
        (
            c.rename().supports(RenameMode::Replace),
            p::Capability::RenameReplace,
        ),
        (c.permissions().can_read(), p::Capability::PermissionsRead),
        (c.permissions().can_write(), p::Capability::PermissionsWrite),
        (c.xattrs().can_read(), p::Capability::XattrsRead),
        (c.xattrs().can_write(), p::Capability::XattrsWrite),
        (
            matches!(c.read(), ReadSupport::Sequential),
            p::Capability::Read,
        ),
        (
            matches!(c.read(), ReadSupport::Range),
            p::Capability::RangeRead,
        ),
        (
            matches!(c.directories(), DirectorySupport::Hierarchical { .. }),
            p::Capability::Hierarchical,
        ),
        (
            matches!(c.directories(), DirectorySupport::Prefix { .. }),
            p::Capability::Prefix,
        ),
    ] {
        if enabled {
            names.push(name.into());
        }
    }
    names
}

pub fn decode_capabilities(names: &[i32]) -> Result<Capabilities, ContractViolation> {
    use p::Capability as C;
    let set: BTreeSet<_> = names.iter().copied().collect();
    let has = |cap: C| set.contains(&i32::from(cap));
    if set.len() != names.len()
        || (has(C::Prefix) && has(C::Hierarchical))
        || (has(C::RangeRead) && has(C::Read))
        || (has(C::PermissionsWrite) && !has(C::PermissionsRead))
        || (has(C::XattrsWrite) && !has(C::XattrsRead))
    {
        return Err(ContractViolation::InvalidDeclaration);
    }
    let atomic = has(C::AtomicPublish);
    let write = match (has(C::WriteNew), has(C::WriteReplace)) {
        (true, true) => WriteSupport::staged(WriteModes::Both, atomic),
        (true, false) => WriteSupport::staged(WriteModes::CreateNew, atomic),
        (false, true) => WriteSupport::staged(WriteModes::Replace, atomic),
        (false, false) if !atomic => WriteSupport::None,
        _ => return Err(ContractViolation::InvalidDeclaration),
    };
    let create = has(C::CreateDir);
    let persistent = has(C::EmptyDirs);
    let directories = if has(C::Hierarchical) {
        DirectorySupport::hierarchical(create, persistent)
    } else if has(C::Prefix) {
        DirectorySupport::prefix(create, persistent)
    } else if !create && !persistent {
        DirectorySupport::None
    } else {
        return Err(ContractViolation::InvalidDeclaration);
    };
    let rename = match (has(C::RenameNew), has(C::RenameReplace)) {
        (true, true) => RenameSupport::Both,
        (true, false) => RenameSupport::NoReplace,
        (false, true) => RenameSupport::Replace,
        (false, false) => RenameSupport::None,
    };
    let attrs = |read, write| match (has(read), has(write)) {
        (_, true) => AttributeSupport::ReadWrite,
        (true, false) => AttributeSupport::Read,
        (false, false) => AttributeSupport::None,
    };
    let read = if has(C::RangeRead) {
        ReadSupport::Range
    } else if has(C::Read) {
        ReadSupport::Sequential
    } else {
        ReadSupport::None
    };
    Ok(Capabilities::default()
        .with_stat(has(C::Stat))
        .with_list(has(C::List))
        .with_read_link(has(C::ReadLink))
        .with_read(read)
        .with_write(write)
        .with_directories(directories)
        .with_remove(has(C::Remove))
        .with_rename(rename)
        .with_permissions(attrs(C::PermissionsRead, C::PermissionsWrite))
        .with_xattrs(attrs(C::XattrsRead, C::XattrsWrite)))
}
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EntryValueError {
    #[error("invalid entry address")]
    Path(#[from] PathError),
    #[error("invalid entry metadata")]
    Entry(#[from] EntryError),
    #[error("invalid entry kind or size")]
    Shape,
    #[error("entry timestamp is outside the representable range or precision")]
    Timestamp,
}

const NANOS: i128 = 1_000_000_000;

pub fn encode_time(time: SystemTime) -> Result<(i64, u32), EntryValueError> {
    let signed = |duration: Duration| {
        i128::from(duration.as_secs()) * NANOS + i128::from(duration.subsec_nanos())
    };
    let total = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => signed(duration),
        Err(error) => -signed(error.duration()),
    };
    Ok((
        i64::try_from(total.div_euclid(NANOS)).map_err(|_| EntryValueError::Timestamp)?,
        u32::try_from(total.rem_euclid(NANOS)).map_err(|_| EntryValueError::Timestamp)?,
    ))
}

pub fn decode_time(seconds: i64, nanos: u32) -> Result<SystemTime, EntryValueError> {
    let invalid = || EntryValueError::Timestamp;
    if i128::from(nanos) >= NANOS {
        return Err(invalid());
    }
    let total = i128::from(seconds) * NANOS + i128::from(nanos);
    let duration = Duration::new(
        u64::try_from(total.abs() / NANOS).map_err(|_| invalid())?,
        u32::try_from(total.abs() % NANOS).map_err(|_| invalid())?,
    );
    let time = if total < 0 {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    }
    .ok_or_else(invalid)?;
    if encode_time(time)? != (seconds, nanos) {
        return Err(invalid());
    }
    Ok(time)
}

pub fn encode_entry(entry: &Entry) -> Result<p::Entry, EntryValueError> {
    let (kind, size) = match entry.entry_type() {
        EntryType::File { size, .. } => (p::EntryKind::File, size),
        EntryType::Directory => (p::EntryKind::Dir, None),
        EntryType::Symlink => (p::EntryKind::Symlink, None),
        EntryType::Other => (p::EntryKind::Other, None),
        _ => return Err(EntryValueError::Shape),
    };
    let modified = entry
        .modified()
        .map(encode_time)
        .transpose()?
        .map(|(seconds, nanos)| p::Timestamp { seconds, nanos });
    Ok(p::Entry {
        path: entry.path().as_str().into(),
        kind: kind.into(),
        size,
        modified,
    })
}

pub fn decode_entry(entry: p::Entry) -> Result<Entry, EntryValueError> {
    let kind = match (p::EntryKind::try_from(entry.kind), entry.size) {
        (Ok(p::EntryKind::File), size) => EntryType::file(size),
        (Ok(p::EntryKind::Dir), None) => EntryType::Directory,
        (Ok(p::EntryKind::Symlink), None) => EntryType::Symlink,
        (Ok(p::EntryKind::Other), None) => EntryType::Other,
        _ => return Err(EntryValueError::Shape),
    };
    let modified = entry
        .modified
        .map(|time| decode_time(time.seconds, time.nanos))
        .transpose()?;
    Ok(Entry::new(
        StoragePath::parse(&entry.path)?,
        kind,
        modified,
    )?)
}
