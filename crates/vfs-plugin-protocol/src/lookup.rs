use super::{FinalLink, IntermediateLink, Lookup, Read, Stat, Write};
use ah_vfs_storage::{
    IntermediateSymlinks, PathError, PathExpr, PathExprError, Publication, RangeError, ReadRange,
    ReadRequest, StatRequest, StoragePath, SymlinkAction, WriteMode, WriteRequest,
};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LookupValueError {
    #[error("missing lookup")]
    Missing,
    #[error("invalid path operand")]
    Operand(#[from] PathExprError),
    #[error("invalid working directory")]
    WorkingDirectory(#[from] PathError),
    #[error("invalid symbolic link policy")]
    Policy,
    #[error("invalid read range")]
    Range(#[from] RangeError),
    #[error("invalid write mode or publication guarantee")]
    WritePolicy,
    #[error("invalid directory creation mode")]
    CreateDirectoryMode,
}

pub fn encode_lookup(
    operand: &PathExpr,
    working_directory: &StoragePath,
    links: IntermediateSymlinks,
) -> Lookup {
    Lookup {
        operand: operand.to_string(),
        working_directory: working_directory.as_str().into(),
        intermediate_links: match links {
            IntermediateSymlinks::FollowWithinInstance => IntermediateLink::FollowIntermediate,
            IntermediateSymlinks::Reject => IntermediateLink::RejectIntermediate,
        }
        .into(),
    }
}

pub fn decode_lookup(
    value: Option<Lookup>,
) -> Result<(PathExpr, StoragePath, IntermediateSymlinks), LookupValueError> {
    let value = value.ok_or(LookupValueError::Missing)?;
    let links = match IntermediateLink::try_from(value.intermediate_links) {
        Ok(IntermediateLink::FollowIntermediate) => IntermediateSymlinks::FollowWithinInstance,
        Ok(IntermediateLink::RejectIntermediate) => IntermediateSymlinks::Reject,
        _ => return Err(LookupValueError::Policy),
    };
    Ok((
        PathExpr::parse(&value.operand)?,
        StoragePath::parse(&value.working_directory)?,
        links,
    ))
}

pub fn encode_stat(request: &StatRequest, base: &StoragePath, links: IntermediateSymlinks) -> Stat {
    Stat {
        lookup: Some(encode_lookup(request.path(), base, links)),
        final_link: match request.lookup().final_symlink() {
            SymlinkAction::FollowWithinInstance => FinalLink::FollowFinal,
            SymlinkAction::Preserve => FinalLink::PreserveFinal,
        }
        .into(),
    }
}

pub fn decode_stat(
    value: Stat,
) -> Result<(StatRequest, StoragePath, IntermediateSymlinks), LookupValueError> {
    let (operand, base, links) = decode_lookup(value.lookup)?;
    let final_link = match FinalLink::try_from(value.final_link) {
        Ok(FinalLink::FollowFinal) => SymlinkAction::FollowWithinInstance,
        Ok(FinalLink::PreserveFinal) => SymlinkAction::Preserve,
        _ => return Err(LookupValueError::Policy),
    };
    Ok((StatRequest::new(operand, final_link), base, links))
}

pub fn encode_read(
    handle: u64,
    request: &ReadRequest,
    base: &StoragePath,
    links: IntermediateSymlinks,
) -> Read {
    Read {
        handle,
        lookup: Some(encode_lookup(request.path(), base, links)),
        offset: request.range().offset(),
        length: request.range().length(),
    }
}

pub fn decode_read(
    value: Read,
) -> Result<(ReadRequest, StoragePath, IntermediateSymlinks), LookupValueError> {
    let range = ReadRange::new(value.offset, value.length)?;
    let (operand, base, links) = decode_lookup(value.lookup)?;
    Ok((ReadRequest::new(operand, range), base, links))
}

pub fn encode_write(
    handle: u64,
    request: &WriteRequest,
    base: &StoragePath,
    links: IntermediateSymlinks,
) -> Write {
    Write {
        handle,
        lookup: Some(encode_lookup(request.path(), base, links)),
        mode: match request.mode() {
            WriteMode::CreateNew => super::WriteMode::CreateNew,
            WriteMode::Replace => super::WriteMode::Replace,
        }
        .into(),
        publication: match request.publication() {
            Publication::Staged => super::Publication::Staged,
            Publication::Atomic => super::Publication::Atomic,
        }
        .into(),
    }
}

pub fn decode_write(
    value: Write,
) -> Result<(WriteRequest, StoragePath, IntermediateSymlinks), LookupValueError> {
    let mode = match super::WriteMode::try_from(value.mode) {
        Ok(super::WriteMode::CreateNew) => WriteMode::CreateNew,
        Ok(super::WriteMode::Replace) => WriteMode::Replace,
        _ => return Err(LookupValueError::WritePolicy),
    };
    let publication = match super::Publication::try_from(value.publication) {
        Ok(super::Publication::Staged) => Publication::Staged,
        Ok(super::Publication::Atomic) => Publication::Atomic,
        _ => return Err(LookupValueError::WritePolicy),
    };
    let (operand, base, links) = decode_lookup(value.lookup)?;
    Ok((WriteRequest::new(operand, mode, publication), base, links))
}

pub fn encode_create_directory(
    request: &ah_vfs_storage::CreateDirectoryRequest,
    base: &StoragePath,
    links: IntermediateSymlinks,
) -> super::CreateDirectory {
    use ah_vfs_storage::CreateDirectoryMode;
    let mode = match request.mode() {
        CreateDirectoryMode::Single => super::CreateDirectoryMode::Single,
        CreateDirectoryMode::Recursive => super::CreateDirectoryMode::Recursive,
    };
    super::CreateDirectory {
        lookup: Some(encode_lookup(request.path(), base, links)),
        mode: mode.into(),
    }
}

pub fn decode_create_directory(
    value: super::CreateDirectory,
) -> Result<
    (
        ah_vfs_storage::CreateDirectoryRequest,
        StoragePath,
        IntermediateSymlinks,
    ),
    LookupValueError,
> {
    use ah_vfs_storage::{CreateDirectoryMode, CreateDirectoryRequest};
    let mode = match super::CreateDirectoryMode::try_from(value.mode) {
        Ok(super::CreateDirectoryMode::Single) => CreateDirectoryMode::Single,
        Ok(super::CreateDirectoryMode::Recursive) => CreateDirectoryMode::Recursive,
        _ => return Err(LookupValueError::CreateDirectoryMode),
    };
    let (operand, base, links) = decode_lookup(value.lookup)?;
    Ok((CreateDirectoryRequest::new(operand, mode), base, links))
}
