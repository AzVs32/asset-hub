use crate::bound::ExecutionError as LookupValueError;
use ah_vfs_storage::{Publication, WriteMode};

pub(crate) fn decode_write_policy(
    mode: i32,
    publication: i32,
) -> Result<(WriteMode, Publication), LookupValueError> {
    let mode = match super::WriteMode::try_from(mode) {
        Ok(super::WriteMode::CreateNew) => WriteMode::CreateNew,
        Ok(super::WriteMode::Replace) => WriteMode::Replace,
        _ => return Err(LookupValueError::Policy),
    };
    let publication = match super::Publication::try_from(publication) {
        Ok(super::Publication::Staged) => Publication::Staged,
        Ok(super::Publication::Atomic) => Publication::Atomic,
        _ => return Err(LookupValueError::Policy),
    };
    Ok((mode, publication))
}

pub(crate) fn decode_remove_kind(
    kind: i32,
) -> Result<ah_vfs_storage::RemoveKind, LookupValueError> {
    let kind = match super::remove::Kind::try_from(kind) {
        Ok(super::remove::Kind::NonDirectory) => ah_vfs_storage::RemoveKind::NonDirectory,
        Ok(super::remove::Kind::EmptyDirectory) => ah_vfs_storage::RemoveKind::EmptyDirectory,
        _ => return Err(LookupValueError::Policy),
    };
    Ok(kind)
}

pub(crate) fn decode_rename_mode(
    mode: i32,
) -> Result<ah_vfs_storage::RenameMode, LookupValueError> {
    let mode = match super::rename::Mode::try_from(mode) {
        Ok(super::rename::Mode::NoReplace) => ah_vfs_storage::RenameMode::NoReplace,
        Ok(super::rename::Mode::Replace) => ah_vfs_storage::RenameMode::Replace,
        _ => return Err(LookupValueError::Policy),
    };
    Ok(mode)
}

/// Encode the opaque link bytes without a second allocation.
pub fn encode_link_target(
    target: ah_vfs_storage::LinkTarget,
) -> Result<super::Reply, ah_vfs_storage::Rejection> {
    let data = target.into_bytes();
    if data.len() > super::MAX_DATA_BYTES {
        return Err(ah_vfs_storage::Rejection::LimitExceeded);
    }
    Ok(super::Reply {
        result: Some(super::Value::Bytes(super::Empty {})),
        data,
    })
}
