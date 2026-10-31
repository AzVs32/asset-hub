//! Windows handle operations. Namespace mutations never fall back to copy/delete
//! or re-resolve an absolute path after opening the parent directory.
use super::{TEMP_PREFIX, fs};
use ah_vfs::domain::{StorageError, StorageErrorKind};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions, OpenOptionsExt};
use std::{io, mem::{offset_of, size_of}, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
        ACL, PSECURITY_DESCRIPTOR, GetSecurityDescriptorControl,
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        UNPROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
    },
    Storage::FileSystem::{
        DELETE, FILE_READ_ATTRIBUTES, SYNCHRONIZE,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_SHARE_DELETE,
        FILE_RENAME_INFO, FILE_RENAME_INFO_0, FILE_DISPOSITION_INFO,
        FileRenameInfo, FileDispositionInfo, SetFileInformationByHandle,
        GetFinalPathNameByHandleW,
    },
};

pub(super) fn open_public(parent: &Dir, name: &str, access: u32) -> Result<File, StorageError> {
    let mut options = OpenOptions::new();
    options.read(true).access_mode(access | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .follow(FollowSymlinks::No);
    let file = parent.open_with(name, &options).map_err(fs::error)?;
    public_handle(&file)?;
    Ok(file)
}

/// Check the normalized long name as well as the requested name, preventing DOS
/// 8.3 aliases from exposing the reserved staging namespace.
pub(super) fn public_handle(file: &File) -> Result<(), StorageError> {
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the live file owns the handle; buffer contains the advertised
        // number of initialized UTF-16 elements and remains live for the call.
        let length = unsafe {
            GetFinalPathNameByHandleW(file.as_raw_handle(), buffer.as_mut_ptr(), buffer.len() as u32, 0)
        };
        if length == 0 { return Err(fs::error(io::Error::last_os_error())); }
        if length as usize >= buffer.len() {
            if length > 32_768 { return Err(StorageError::new(StorageErrorKind::InvalidPath)); }
            buffer.resize(length as usize + 1, 0);
            continue;
        }
        let path = String::from_utf16(&buffer[..length as usize])
            .map_err(|_| StorageError::new(StorageErrorKind::InvalidNameEncoding))?;
        let name = path.rsplit(['\\', '/']).next().unwrap_or("");
        if name.to_ascii_lowercase().starts_with(TEMP_PREFIX) {
            return Err(StorageError::new(StorageErrorKind::PermissionDenied));
        }
        return Ok(());
    }
}

pub(super) fn rename(file: &File, destination: &Dir, name: &str, replace: bool) -> Result<(), StorageError> {
    fs::validate_name(name)?;
    let name: Vec<u16> = name.encode_utf16().collect();
    let name_bytes = name.len().checked_mul(size_of::<u16>())
        .ok_or_else(|| StorageError::new(StorageErrorKind::InvalidPath))?;
    let length = offset_of!(FILE_RENAME_INFO, FileName).checked_add(name_bytes)
        .ok_or_else(|| StorageError::new(StorageErrorKind::InvalidPath))?
        .max(size_of::<FILE_RENAME_INFO>());
    let length32 = u32::try_from(length).map_err(|_| StorageError::new(StorageErrorKind::InvalidPath))?;
    // usize storage supplies alignment for the Windows pointer-bearing header.
    let mut buffer = vec![0usize; length.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: the aligned allocation holds the complete header and counted UTF-16
    // tail. Both handles remain owned by the caller. No terminating NUL is needed.
    let result = unsafe {
        ptr::addr_of_mut!((*info).Anonymous).write(FILE_RENAME_INFO_0 { ReplaceIfExists: replace });
        ptr::addr_of_mut!((*info).RootDirectory).write(destination.as_raw_handle());
        ptr::addr_of_mut!((*info).FileNameLength).write(name_bytes as u32);
        ptr::copy_nonoverlapping(name.as_ptr(), ptr::addr_of_mut!((*info).FileName).cast::<u16>(), name.len());
        SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, info.cast(), length32)
    };
    if result == 0 { return Err(fs::error(io::Error::last_os_error())); }
    Ok(())
}

pub(super) fn delete(file: &File) -> Result<(), StorageError> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the structure and DELETE-capable live handle remain valid for this
    // synchronous call. Windows rejects nonempty directories; no recursive delete.
    let result = unsafe {
        SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo,
            ptr::from_ref(&info).cast(), size_of::<FILE_DISPOSITION_INFO>() as u32)
    };
    if result == 0 { return Err(fs::error(io::Error::last_os_error())); }
    Ok(())
}

struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetSecurityInfo allocated this descriptor with LocalAlloc.
            unsafe { LocalFree(self.0); }
        }
    }
}

/// Preserve destination access control before publishing a replacement. Missing
/// READ_CONTROL/WRITE_DAC rights fail before publication instead of widening access.
pub(super) fn copy_dacl(source: &File, staged: &File) -> Result<(), StorageError> {
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: handles are live and all output pointers reference initialized locals.
    let result = unsafe {
        GetSecurityInfo(source.as_raw_handle(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION,
            ptr::null_mut(), ptr::null_mut(), &mut dacl, ptr::null_mut(), &mut descriptor)
    };
    let descriptor = Descriptor(descriptor);
    if result != 0 { return Err(fs::error(io::Error::from_raw_os_error(result as i32))); }
    if descriptor.0.is_null() { return Err(StorageError::new(StorageErrorKind::InvalidData)); }
    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: descriptor owns the security descriptor until the function returns.
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
        return Err(fs::error(io::Error::last_os_error()));
    }
    let inheritance = if control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    // SAFETY: dacl is either null (a valid null DACL) or points into the owned
    // descriptor. The staged handle was opened with WRITE_DAC.
    let result = unsafe {
        SetSecurityInfo(staged.as_raw_handle(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION | inheritance,
            ptr::null_mut(), ptr::null_mut(), dacl, ptr::null_mut())
    };
    if result != 0 { return Err(fs::error(io::Error::from_raw_os_error(result as i32))); }
    Ok(())
}

pub(super) fn mutation_handle(parent: &Dir, name: &str) -> Result<File, StorageError> {
    open_public(parent, name, DELETE)
}
