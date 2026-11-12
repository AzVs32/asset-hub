//! POSIX mode bits and bounded xattrs on entries selected by the shared lookup.
use super::{LinuxLocalStorage, fs, lookup};
use crate::driver::LinuxLocalDriver;
use ah_vfs::application::port::StorageAttributes;
use ah_vfs_storage::{
    AttributeAction, AttributeRequest, AttributeResult, AttributeSupport, BackendFailure, Entry,
    EntryType, Failure, Operation, Permissions, Rejection, StorageError, XattrList, XattrName,
    XattrWriteMode,
};
use rustix::fs::{self as native, AtFlags, CWD, Mode, XattrFlags};
use rustix::io::Errno;

const MAX_NATIVE_XATTR_BYTES: usize = 64 * 1024;

impl StorageAttributes for LinuxLocalStorage {
    fn support(&self) -> (AttributeSupport, AttributeSupport) {
        let caps = LinuxLocalDriver::supported_capabilities();
        (caps.permissions(), caps.xattrs())
    }
}
pub(super) fn execute(
    entry: &Entry,
    handle: &cap_std::fs::File,
    request: AttributeRequest,
) -> Result<AttributeResult, StorageError> {
    let op = request.operation();
    if !LinuxLocalDriver::supported_capabilities().supports(op) {
        return Err(rejected(op, Rejection::Unsupported));
    }
    if matches!(request.action(), AttributeAction::SetXattr { value, .. } if value.len() > MAX_NATIVE_XATTR_BYTES)
    {
        return Err(rejected(op, Rejection::LimitExceeded));
    }
    let (_, action) = request.into_parts();
    if let AttributeAction::GetPermissions = action {
        let stat = native::fstat(handle).map_err(|error| native_error(op, error))?;
        let bits = Mode::from_raw_mode(stat.st_mode).as_raw_mode();
        return Permissions::new(bits)
            .map(AttributeResult::Permissions)
            .map_err(|cause| {
                StorageError::without_effects(
                    op,
                    Failure::Backend(BackendFailure::CorruptData),
                    Some(Box::new(cause)),
                )
            });
    }
    if !matches!(
        entry.entry_type(),
        EntryType::Directory | EntryType::File { .. }
    ) {
        // These operations have no supported handle-based implementation for
        // the selected link or special entry. Never operate on its referent.
        return Err(rejected(op, Rejection::Unsupported));
    }
    // This procfs reference follows the held descriptor, not the original
    // name. The handle stays live throughout the native call.
    let bound = lookup::descriptor_path(handle);
    let bound = bound.as_str();
    match action {
        AttributeAction::SetPermissions(mode) => {
            native::chmodat(
                CWD,
                bound,
                Mode::from_raw_mode(mode.bits()),
                AtFlags::empty(),
            )
            .map_err(|error| native_error(op, error))?;
            Ok(AttributeResult::Unit)
        }
        AttributeAction::ListXattrs { max_bytes, .. } => {
            let mut bytes = vec![0; max_bytes.get().min(MAX_NATIVE_XATTR_BYTES)];
            let len = native::listxattr(bound, &mut bytes[..])
                .map_err(|error| native_error(op, error))?;
            let bytes = &bytes[..len];
            let mut names = if bytes.is_empty() {
                Vec::new()
            } else {
                let packed = bytes.strip_suffix(&[0]).ok_or_else(|| corrupt(op))?;
                packed
                    .split(|byte| *byte == 0)
                    .map(|name| {
                        XattrName::from_backend_bytes(name).map_err(|reason| {
                            StorageError::without_effects(op, Failure::Backend(reason), None)
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            names.sort_unstable();
            XattrList::new(names, max_bytes)
                .map(AttributeResult::Names)
                .map_err(|_| corrupt(op))
        }
        AttributeAction::GetXattr {
            name, max_bytes, ..
        } => {
            let mut bytes = vec![0; max_bytes.get().min(MAX_NATIVE_XATTR_BYTES)];
            let len = native::getxattr(bound, name.as_str(), &mut bytes[..])
                .map_err(|error| native_error(op, error))?;
            bytes.truncate(len);
            Ok(AttributeResult::Value(bytes))
        }
        AttributeAction::SetXattr {
            name, value, mode, ..
        } => {
            let flags = match mode {
                XattrWriteMode::Upsert => XattrFlags::empty(),
                XattrWriteMode::CreateNew => XattrFlags::CREATE,
                XattrWriteMode::Replace => XattrFlags::REPLACE,
            };
            native::setxattr(bound, name.as_str(), &value, flags)
                .map_err(|error| native_error(op, error))?;
            Ok(AttributeResult::Unit)
        }
        AttributeAction::RemoveXattr(name) => {
            native::removexattr(bound, name.as_str()).map_err(|error| native_error(op, error))?;
            Ok(AttributeResult::Unit)
        }
        _ => Err(rejected(op, Rejection::Unsupported)),
    }
}

fn rejected(op: Operation, reason: Rejection) -> StorageError {
    StorageError::without_effects(op, Failure::Rejected(reason), None)
}

fn corrupt(op: Operation) -> StorageError {
    StorageError::without_effects(op, Failure::Backend(BackendFailure::CorruptData), None)
}

fn native_error(op: Operation, cause: Errno) -> StorageError {
    let reason = match cause {
        Errno::RANGE | Errno::TOOBIG => Rejection::LimitExceeded,
        Errno::NOSYS | Errno::OPNOTSUPP | Errno::NOENT => Rejection::Unsupported,
        Errno::INVAL
            if matches!(
                op,
                Operation::GetXattr | Operation::SetXattr | Operation::RemoveXattr
            ) =>
        {
            Rejection::UnsupportedName
        }
        Errno::ACCESS | Errno::PERM | Errno::ROFS => Rejection::AccessDenied,
        Errno::NODATA => Rejection::AttributeNotFound,
        Errno::EXIST => Rejection::AlreadyExists,
        Errno::NOSPC | Errno::DQUOT => return fs::error(op, cause.into()),
        _ => return fs::dispatched_error(op, cause.into()),
    };
    StorageError::without_effects(
        op,
        Failure::Rejected(reason),
        Some(Box::new(std::io::Error::from(cause))),
    )
}
