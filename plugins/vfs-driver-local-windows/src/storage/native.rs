//! Windows handle operations. Namespace mutations never fall back to copy/delete
//! or re-resolve an absolute path after opening the parent directory.
use super::{TEMP_PREFIX, fs};
use ah_vfs_storage::{BackendFailure, Failure, Operation, Rejection, StorageError};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{
    io,
    mem::{offset_of, size_of},
    os::windows::io::AsRawHandle,
    ptr,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_RENAME_INFO_0, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FileDispositionInfo, FileRenameInfo, GetFinalPathNameByHandleW, SYNCHRONIZE,
    SetFileInformationByHandle,
};

pub(super) fn open_public(
    operation: Operation,
    parent: &Dir,
    name: &str,
    access: u32,
) -> Result<File, StorageError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .access_mode(access | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let file = parent
        .open_with(name, &options)
        .map_err(|cause| fs::error(operation, cause))?;
    public_handle(operation, &file)?;
    Ok(file)
}

/// Check the normalized long name as well as the requested name, preventing DOS
/// 8.3 aliases from exposing the reserved staging namespace.
pub(super) fn public_handle(operation: Operation, file: &File) -> Result<(), StorageError> {
    actual_name(operation, file).map(drop)
}

pub(super) fn actual_name(operation: Operation, file: &File) -> Result<String, StorageError> {
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the live file owns the handle; buffer contains the advertised
        // number of initialized UTF-16 elements and remains live for the call.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        };
        if length == 0 {
            return Err(fs::error(operation, io::Error::last_os_error()));
        }
        if length as usize >= buffer.len() {
            if length > 32_768 {
                return Err(StorageError::without_effects(
                    operation,
                    Failure::Rejected(Rejection::UnsupportedName),
                    None,
                ));
            }
            buffer.resize(length as usize + 1, 0);
            continue;
        }
        let path = String::from_utf16(&buffer[..length as usize]).map_err(|_| {
            StorageError::without_effects(
                operation,
                Failure::Backend(BackendFailure::UnrepresentableName),
                None,
            )
        })?;
        let name = path.rsplit(['\\', '/']).next().unwrap_or("");
        if name.to_ascii_lowercase().starts_with(TEMP_PREFIX) {
            return Err(StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::AccessDenied),
                None,
            ));
        }
        return Ok(name.to_owned());
    }
}

pub(super) fn rename(
    operation: Operation,
    file: &File,
    destination: &Dir,
    name: &str,
) -> Result<(), StorageError> {
    fs::validate_name(operation, name)?;
    let name: Vec<u16> = name.encode_utf16().collect();
    let name_bytes = name.len().checked_mul(size_of::<u16>()).ok_or_else(|| {
        StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::UnsupportedName),
            None,
        )
    })?;
    let length = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes)
        .ok_or_else(|| {
            StorageError::without_effects(
                operation,
                Failure::Rejected(Rejection::UnsupportedName),
                None,
            )
        })?
        .max(size_of::<FILE_RENAME_INFO>());
    let length32 = u32::try_from(length).map_err(|_| {
        StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::UnsupportedName),
            None,
        )
    })?;
    // usize storage supplies alignment for the Windows pointer-bearing header.
    let mut buffer = vec![0usize; length.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: the aligned allocation holds the complete header and counted UTF-16
    // tail. Both handles remain owned by the caller. No terminating NUL is needed.
    let result = unsafe {
        ptr::addr_of_mut!((*info).Anonymous).write(FILE_RENAME_INFO_0 {
            ReplaceIfExists: false,
        });
        ptr::addr_of_mut!((*info).RootDirectory).write(destination.as_raw_handle());
        ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes as u32);
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            name.len(),
        );
        SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, info.cast(), length32)
    };
    if result == 0 {
        return Err(mutation_error(operation, io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn delete(operation: Operation, file: &File) -> Result<(), StorageError> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the structure and DELETE-capable live handle remain valid for this
    // synchronous call. Windows rejects nonempty directories; no recursive delete.
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            ptr::from_ref(&info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(fs::error(operation, io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn remove(file: &File) -> Result<(), StorageError> {
    use windows_sys::Win32::{
        Foundation::{ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
        Storage::FileSystem::{
            FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
            FILE_DISPOSITION_INFO_EX, FileDispositionInfoEx,
        },
    };
    let op = Operation::Remove;
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    // SAFETY: the DELETE-capable handle and disposition data stay live for the call.
    // Closing this handle removes the name even while read handles remain open.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfoEx,
            ptr::from_ref(&info).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    } == 0
    {
        let cause = io::Error::last_os_error();
        if matches!(
            cause.raw_os_error().map(|code| code as u32),
            Some(ERROR_INVALID_FUNCTION | ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED)
        ) {
            return Err(StorageError::without_effects(
                op,
                Failure::Rejected(Rejection::Unsupported),
                Some(Box::new(cause)),
            ));
        }
        return Err(mutation_error(op, cause));
    }
    Ok(())
}

pub(super) fn mutation_handle(
    operation: Operation,
    parent: &Dir,
    name: &str,
) -> Result<File, StorageError> {
    fs::validate_name(operation, name)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .access_mode(DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let file = parent
        .open_with(name, &options)
        .map_err(|cause| fs::error(operation, cause))?;
    if actual_name(operation, &file)? != name {
        return Err(StorageError::without_effects(
            operation,
            Failure::Rejected(Rejection::NotFound),
            None,
        ));
    }
    Ok(file)
}

fn mutation_error(operation: Operation, cause: io::Error) -> StorageError {
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_DIR_NOT_EMPTY, ERROR_FILE_EXISTS,
        ERROR_FILE_NOT_FOUND, ERROR_NOT_SAME_DEVICE, ERROR_NOT_SUPPORTED, ERROR_PATH_NOT_FOUND,
        ERROR_SHARING_VIOLATION,
    };
    match cause.raw_os_error().map(|code| code as u32) {
        Some(
            ERROR_ACCESS_DENIED
            | ERROR_ALREADY_EXISTS
            | ERROR_DIR_NOT_EMPTY
            | ERROR_FILE_EXISTS
            | ERROR_FILE_NOT_FOUND
            | ERROR_NOT_SAME_DEVICE
            | ERROR_NOT_SUPPORTED
            | ERROR_PATH_NOT_FOUND
            | ERROR_SHARING_VIOLATION,
        ) => fs::error(operation, cause),
        _ => fs::dispatched_error(operation, cause),
    }
}
