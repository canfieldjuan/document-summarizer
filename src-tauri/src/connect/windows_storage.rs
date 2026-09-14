use std::ffi::{c_void, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf};
use std::ptr::{null, null_mut};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION,
    ERROR_SHARING_VIOLATION, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AclSizeInformation, GetAce, GetAclInformation, GetLengthSid, GetSecurityDescriptorControl,
    GetSecurityDescriptorDacl, GetTokenInformation, IsValidSid, TokenUser, ACL,
    ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, INHERIT_ONLY_ACE, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
    TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileAttributeTagInfo, GetFileInformationByHandleEx, LockFileEx, MoveFileExW,
    UnlockFileEx, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA,
    LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, OPEN_EXISTING, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::System::IO::OVERLAPPED;

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0x00;
const ACCESS_ALLOWED_COMPOUND_ACE_TYPE: u8 = 0x04;
const ACCESS_ALLOWED_OBJECT_ACE_TYPE: u8 = 0x05;
const ACCESS_ALLOWED_CALLBACK_ACE_TYPE: u8 = 0x09;
const ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE: u8 = 0x0b;
const ACE_OBJECT_TYPE_PRESENT: u32 = 0x01;
const ACE_INHERITED_OBJECT_TYPE_PRESENT: u32 = 0x02;
const SDDL_REVISION_1: u32 = 1;
const FILE_OPERATION_ATTEMPTS: usize = 20;
const FILE_OPERATION_DELAY: Duration = Duration::from_millis(25);
const LOCK_OFFSET: u64 = 0;
const LOCK_LENGTH: u32 = 1;
const SENSITIVE_ACCESS: u32 = FILE_READ_DATA
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_WRITE_EA
    | FILE_DELETE_CHILD
    | FILE_WRITE_ATTRIBUTES
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | GENERIC_ALL
    | GENERIC_WRITE
    | GENERIC_READ;

const SYSTEM_SID: &str = "S-1-5-18";
const ADMINISTRATORS_SID: &str = "S-1-5-32-544";
const CREATOR_OWNER_SID: &str = "S-1-3-0";
const OWNER_RIGHTS_SID: &str = "S-1-3-4";

pub(crate) const LOCAL_CONNECT_DIRECTORY: &str = "LocalConnect";

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum FileLockError {
    Busy,
    Io(io::Error),
}

impl From<io::Error> for FileLockError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) struct WindowsFileLock {
    file: File,
}

impl WindowsFileLock {
    pub(crate) fn acquire(path: &Path, private_root: &Path) -> Result<Self, FileLockError> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Windows lock has no parent")
        })?;
        validate_private_directory(parent, private_root)?;

        let existed = path_entry_exists(path)?;
        if existed {
            validate_private_regular_file(path, private_root)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(path)?;
        if !existed {
            protect_path(path, false)?;
        }
        validate_opened_handle(&file, false, true, true)?;
        if file.metadata()?.len() == 0 {
            file.write_all(&[0])?;
            file.sync_all()?;
        }
        file.seek(SeekFrom::Start(LOCK_OFFSET))?;
        let mut overlapped = OVERLAPPED::default();
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle() as HANDLE,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                LOCK_LENGTH,
                0,
                &mut overlapped,
            )
        };
        if locked == 0 {
            let code = unsafe { GetLastError() };
            return if matches!(code, ERROR_LOCK_VIOLATION | ERROR_SHARING_VIOLATION) {
                Err(FileLockError::Busy)
            } else {
                Err(FileLockError::Io(io::Error::from_raw_os_error(code as i32)))
            };
        }
        Ok(Self { file })
    }
}

impl Drop for WindowsFileLock {
    fn drop(&mut self) {
        let mut overlapped = OVERLAPPED::default();
        unsafe {
            UnlockFileEx(
                self.file.as_raw_handle() as HANDLE,
                0,
                LOCK_LENGTH,
                0,
                &mut overlapped,
            );
        }
    }
}

pub(crate) fn local_app_data_root(value: Option<OsString>) -> io::Result<PathBuf> {
    let value = value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is required"))?;
    let root = PathBuf::from(value);
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOCALAPPDATA must be absolute",
        ));
    }
    validate_existing_path(&root, true, false)?;
    Ok(root)
}

pub(crate) fn prepare_local_connect_root(local_app_data: &Path) -> io::Result<PathBuf> {
    validate_existing_path(local_app_data, true, false)?;
    let root = local_app_data.join(LOCAL_CONNECT_DIRECTORY);
    ensure_private_directory(&root, local_app_data)?;
    Ok(root)
}

pub(crate) fn ensure_private_directory(path: &Path, private_root: &Path) -> io::Result<()> {
    validate_existing_path(private_root, true, false)?;
    let relative = path.strip_prefix(private_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows private directory escapes its root",
        )
    })?;
    let mut current = private_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows private directory has an unsafe component",
            ));
        };
        current.push(component);
        match fs::create_dir(&current) {
            Ok(()) => protect_path(&current, true)?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        validate_existing_path(&current, true, true)?;
    }
    Ok(())
}

pub(crate) fn validate_private_directory(path: &Path, private_root: &Path) -> io::Result<()> {
    validate_existing_path(private_root, true, false)?;
    let relative = path.strip_prefix(private_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows private directory escapes its root",
        )
    })?;
    let mut current = private_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Windows private directory has an unsafe component",
            ));
        };
        current.push(component);
        validate_existing_path(&current, true, true)?;
    }
    Ok(())
}

pub(crate) fn validate_private_regular_file(path: &Path, private_root: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Windows file has no parent"))?;
    validate_private_directory(parent, private_root)?;
    validate_existing_path(path, false, true)
}

pub(crate) fn read_bounded_regular_file(
    path: &Path,
    maximum: u64,
    allow_empty: bool,
    private_root: Option<&Path>,
) -> io::Result<Vec<u8>> {
    if let Some(root) = private_root {
        validate_private_regular_file(path, root)?;
    }
    let mut file = open_existing(path, false, GENERIC_READ | READ_CONTROL)?;
    validate_opened_handle(&file, false, private_root.is_some(), private_root.is_some())?;
    let length = file.metadata()?.len();
    if length > maximum || (!allow_empty && length == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows file is empty or oversized",
        ));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum || (!allow_empty && bytes.is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows file changed while it was read",
        ));
    }
    Ok(bytes)
}

pub(crate) fn path_entry_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(crate) fn atomic_replace_bytes(
    destination: &Path,
    bytes: &[u8],
    maximum: u64,
    allow_empty: bool,
    private_root: &Path,
) -> io::Result<()> {
    if bytes.len() as u64 > maximum || (!allow_empty && bytes.is_empty()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows replacement content is empty or oversized",
        ));
    }
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows replacement has no parent",
        )
    })?;
    validate_private_directory(parent, private_root)?;
    if path_entry_exists(destination)? {
        validate_private_regular_file(destination, private_root)?;
    }
    let filename = destination.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows replacement has no filename",
        )
    })?;
    let mut temporary_name = OsString::from(".");
    temporary_name.push(filename);
    temporary_name.push(".tmp");
    let temporary = parent.join(temporary_name);
    if path_entry_exists(&temporary)? {
        validate_private_regular_file(&temporary, private_root)?;
        retry_file_operation(|| fs::remove_file(&temporary))?;
    }

    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ)
            .open(&temporary)?;
        protect_path(&temporary, false)?;
        validate_opened_handle(&file, false, true, true)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        retry_file_operation(|| move_file_replace(&temporary, destination))?;
        validate_private_regular_file(destination, private_root)
    })();
    if result.is_err() && path_entry_exists(&temporary).unwrap_or(false) {
        let _ = validate_private_regular_file(&temporary, private_root)
            .and_then(|()| retry_file_operation(|| fs::remove_file(&temporary)));
    }
    result
}

pub(crate) fn remove_private_file(path: &Path, private_root: &Path) -> io::Result<bool> {
    if !path_entry_exists(path)? {
        return Ok(false);
    }
    validate_private_regular_file(path, private_root)?;
    retry_file_operation(|| fs::remove_file(path))?;
    Ok(true)
}

fn retry_file_operation(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    for attempt in 0..FILE_OPERATION_ATTEMPTS {
        match operation() {
            Ok(()) => return Ok(()),
            Err(error) if is_sharing_error(&error) && attempt + 1 < FILE_OPERATION_ATTEMPTS => {
                thread::sleep(FILE_OPERATION_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded Windows retry loop always returns")
}

fn is_sharing_error(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(|value| value as u32),
        Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
    )
}

fn move_file_replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(source.as_os_str());
    let destination = wide(destination.as_os_str());
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn open_existing(path: &Path, directory: bool, access: u32) -> io::Result<File> {
    let path = wide(path.as_os_str());
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            flags,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle as _) })
}

fn validate_existing_path(path: &Path, directory: bool, require_protected: bool) -> io::Result<()> {
    let file = open_existing(path, directory, READ_CONTROL | FILE_READ_ATTRIBUTES)?;
    validate_opened_handle(&file, directory, true, require_protected)
}

fn validate_opened_handle(
    file: &File,
    directory: bool,
    require_private_acl: bool,
    require_protected: bool,
) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.is_dir() != directory
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows path is not the required non-reparse type",
        ));
    }
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    let tag_read = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as HANDLE,
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if tag_read == 0 || tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows reparse-point status is unavailable or unsafe",
        ));
    }
    if require_private_acl {
        validate_private_acl(file.as_raw_handle() as HANDLE, require_protected)?;
    }
    Ok(())
}

fn protect_path(path: &Path, directory: bool) -> io::Result<()> {
    let user = current_user_sid()?;
    let inheritance = if directory { "OICI" } else { "" };
    let descriptor_text = format!(
        "D:P(A;{inheritance};FA;;;SY)(A;{inheritance};FA;;;BA)(A;{inheritance};FA;;;{user})"
    );
    let descriptor_text = wide(OsStr::new(&descriptor_text));
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor_text.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    };
    if converted == 0 || descriptor.is_null() {
        return Err(io::Error::last_os_error());
    }
    let allocation = LocalAllocation(descriptor);
    let mut dacl_present = 0;
    let mut dacl: *mut ACL = null_mut();
    let mut dacl_defaulted = 0;
    let read = unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    };
    if read == 0 || dacl_present == 0 || dacl.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows private DACL is unavailable",
        ));
    }
    let path = wide(path.as_os_str());
    let result = unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr() as *mut u16,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    drop(allocation);
    if result != 0 {
        Err(io::Error::from_raw_os_error(result as i32))
    } else {
        Ok(())
    }
}

fn validate_private_acl(handle: HANDLE, require_protected: bool) -> io::Result<()> {
    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    if descriptor.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows security descriptor is unavailable",
        ));
    }
    let allocation = LocalAllocation(descriptor);
    if owner.is_null() || dacl.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows owner or DACL is unavailable",
        ));
    }
    if require_protected {
        let mut control = 0;
        let mut revision = 0;
        let read = unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
        if read == 0 || control & SE_DACL_PROTECTED == 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Windows private DACL is not protected",
            ));
        }
    }

    let current_user = current_user_sid()?;
    let owner = sid_string(owner)?;
    if !matches!(owner.as_str(), SYSTEM_SID | ADMINISTRATORS_SID) && owner != current_user {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows path owner is not trusted",
        ));
    }

    let mut information = ACL_SIZE_INFORMATION::default();
    let read = unsafe {
        GetAclInformation(
            dacl,
            (&mut information as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    };
    if read == 0 {
        return Err(io::Error::last_os_error());
    }
    for index in 0..information.AceCount {
        let mut ace: *mut c_void = null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return Err(io::Error::last_os_error());
        }
        validate_ace(ace.cast(), &current_user)?;
    }
    drop(allocation);
    Ok(())
}

fn validate_ace(ace: *const u8, current_user: &str) -> io::Result<()> {
    let ace_type = unsafe { *ace };
    let ace_flags = unsafe { *ace.add(1) };
    let ace_size = unsafe { u16::from_le_bytes([*ace.add(2), *ace.add(3)]) } as usize;
    if ace_size < 8 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows ACL entry is malformed",
        ));
    }
    let mask = unsafe { u32::from_le_bytes([*ace.add(4), *ace.add(5), *ace.add(6), *ace.add(7)]) };
    if mask & SENSITIVE_ACCESS == 0 {
        return Ok(());
    }
    if ace_type == ACCESS_ALLOWED_COMPOUND_ACE_TYPE {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows compound allow entry is unsafe",
        ));
    }
    if !matches!(
        ace_type,
        ACCESS_ALLOWED_ACE_TYPE
            | ACCESS_ALLOWED_OBJECT_ACE_TYPE
            | ACCESS_ALLOWED_CALLBACK_ACE_TYPE
            | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE
    ) {
        return Ok(());
    }
    let mut sid_offset = 8usize;
    if matches!(
        ace_type,
        ACCESS_ALLOWED_OBJECT_ACE_TYPE | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE
    ) {
        if ace_size < 12 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Windows object ACL entry is malformed",
            ));
        }
        let flags =
            unsafe { u32::from_le_bytes([*ace.add(8), *ace.add(9), *ace.add(10), *ace.add(11)]) };
        sid_offset = 12;
        if flags & ACE_OBJECT_TYPE_PRESENT != 0 {
            sid_offset += 16;
        }
        if flags & ACE_INHERITED_OBJECT_TYPE_PRESENT != 0 {
            sid_offset += 16;
        }
    }
    if sid_offset >= ace_size {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows ACL trustee is missing",
        ));
    }
    let sid = unsafe { ace.add(sid_offset) as PSID };
    if unsafe { IsValidSid(sid) } == 0
        || sid_offset + unsafe { GetLengthSid(sid) as usize } > ace_size
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows ACL trustee is invalid",
        ));
    }
    let trustee = sid_string(sid)?;
    if matches!(
        trustee.as_str(),
        SYSTEM_SID | ADMINISTRATORS_SID | OWNER_RIGHTS_SID
    ) || trustee == current_user
        || (trustee == CREATOR_OWNER_SID && u32::from(ace_flags) & INHERIT_ONLY_ACE != 0)
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Windows ACL grants sensitive access to an untrusted principal",
        ))
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    struct TokenHandle(HANDLE);
    impl Drop for TokenHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let token = TokenHandle(token);
    let mut required = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
    }
    if required == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u8; required as usize];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*(buffer.as_ptr() as *const TOKEN_USER) };
    sid_string(user.User.Sid)
}

fn sid_string(sid: PSID) -> io::Result<String> {
    let mut value: *mut u16 = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut value) } == 0 || value.is_null() {
        return Err(io::Error::last_os_error());
    }
    let allocation = LocalAllocation(value.cast());
    let mut length = 0usize;
    while unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(value, length) })
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Windows SID is not UTF-16"))?
        .to_ascii_uppercase();
    drop(allocation);
    Ok(result)
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
pub(crate) fn protect_path_for_test(path: &Path, directory: bool) -> io::Result<()> {
    protect_path(path, directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Instant;

    struct PrivateRoot {
        directory: tempfile::TempDir,
    }

    impl PrivateRoot {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            protect_path(directory.path(), true).unwrap();
            Self { directory }
        }

        fn path(&self) -> &Path {
            self.directory.path()
        }
    }

    fn icacls(path: &Path, arguments: &[&str]) {
        let status = Command::new("icacls")
            .arg(path)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn private_chain_and_file_reject_hostile_acl_on_both_sides() {
        let root = PrivateRoot::new();
        assert_eq!(
            local_app_data_root(Some(root.path().as_os_str().to_os_string())).unwrap(),
            root.path()
        );
        let connect_root = prepare_local_connect_root(root.path()).unwrap();
        let nested = connect_root.join("runtime/v2/providers");
        ensure_private_directory(&nested, root.path()).unwrap();
        let registration = nested.join("local-connect-v2-test.json");
        atomic_replace_bytes(&registration, b"{}", 16, false, root.path()).unwrap();
        assert_eq!(
            read_bounded_regular_file(&registration, 16, false, Some(root.path())).unwrap(),
            b"{}"
        );

        icacls(&registration, &["/grant", "*S-1-3-4:R"]);
        assert_eq!(
            read_bounded_regular_file(&registration, 16, false, Some(root.path())).unwrap(),
            b"{}"
        );
        icacls(&registration, &["/grant", "*S-1-1-0:R"]);
        assert!(read_bounded_regular_file(&registration, 16, false, Some(root.path())).is_err());

        let unprotected = nested.join("unprotected.json");
        atomic_replace_bytes(&unprotected, b"{}", 16, false, root.path()).unwrap();
        icacls(&unprotected, &["/inheritance:e"]);
        assert!(validate_private_regular_file(&unprotected, root.path()).is_err());

        let null_dacl = nested.join("null-dacl.json");
        atomic_replace_bytes(&null_dacl, b"{}", 16, false, root.path()).unwrap();
        let mut null_dacl_path = wide(null_dacl.as_os_str());
        let result = unsafe {
            SetNamedSecurityInfoW(
                null_dacl_path.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        assert_eq!(result, 0);
        assert!(validate_private_regular_file(&null_dacl, root.path()).is_err());

        let other = nested.join("other.json");
        atomic_replace_bytes(&other, b"{}", 16, false, root.path()).unwrap();
        icacls(&nested, &["/grant", "*S-1-1-0:R"]);
        assert!(validate_private_regular_file(&other, root.path()).is_err());
    }

    #[test]
    fn safe_unprotected_local_app_data_boundary_gets_protected_connect_children() {
        let parent = PrivateRoot::new();
        let local_app_data = parent.path().join("ambient-local-app-data");
        fs::create_dir(&local_app_data).unwrap();

        assert_eq!(
            local_app_data_root(Some(local_app_data.as_os_str().to_os_string())).unwrap(),
            local_app_data
        );
        let connect_root = prepare_local_connect_root(&local_app_data).unwrap();
        let providers = connect_root.join("runtime/v1/providers");
        ensure_private_directory(&providers, &local_app_data).unwrap();
        validate_private_directory(&providers, &local_app_data).unwrap();
    }

    #[test]
    fn private_chain_rejects_reparse_ancestor_and_relative_root() {
        assert!(local_app_data_root(Some(OsString::from("relative"))).is_err());
        let root = PrivateRoot::new();
        let target = root.path().join("target");
        ensure_private_directory(&target, root.path()).unwrap();
        let junction = root.path().join(LOCAL_CONNECT_DIRECTORY);
        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mklink failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(ensure_private_directory(&junction.join("runtime"), root.path()).is_err());
    }

    #[test]
    fn fixed_temporary_retries_sharing_and_rejects_unsafe_residue() {
        let root = PrivateRoot::new();
        let connect_root = prepare_local_connect_root(root.path()).unwrap();
        let destination = connect_root.join("entitlement-v1.json");
        atomic_replace_bytes(&destination, b"old", 16, false, root.path()).unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&destination)
            .unwrap();
        let (release_tx, release_rx) = mpsc::channel();
        let release = thread::spawn(move || {
            release_rx.recv().unwrap();
            drop(held);
        });
        let wake = thread::spawn(move || {
            thread::sleep(Duration::from_millis(75));
            release_tx.send(()).unwrap();
        });
        let started = Instant::now();
        atomic_replace_bytes(&destination, b"new", 16, false, root.path()).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(50));
        release.join().unwrap();
        wake.join().unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");

        let temporary = connect_root.join(".entitlement-v1.json.tmp");
        fs::create_dir(&temporary).unwrap();
        assert!(atomic_replace_bytes(&destination, b"bad", 16, false, root.path()).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert!(temporary.is_dir());

        let vanished_target = root.path().join("vanished-target");
        fs::create_dir(&vanished_target).unwrap();
        let dangling = connect_root.join("dangling-registration.json");
        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&dangling)
            .arg(&vanished_target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "mklink failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::remove_dir(&vanished_target).unwrap();
        assert!(path_entry_exists(&dangling).unwrap());
        assert!(atomic_replace_bytes(&dangling, b"bad", 16, false, root.path()).is_err());
        assert!(path_entry_exists(&dangling).unwrap());
    }

    #[test]
    fn one_byte_lock_is_nonblocking_and_persistent() {
        let root = PrivateRoot::new();
        let connect_root = prepare_local_connect_root(root.path()).unwrap();
        let locks = connect_root.join("runtime/v1/locks");
        ensure_private_directory(&locks, root.path()).unwrap();
        let path = locks.join(".local-connect-v1-document-summarizer.lock");
        let first = WindowsFileLock::acquire(&path, root.path()).unwrap();
        assert!(matches!(
            WindowsFileLock::acquire(&path, root.path()),
            Err(FileLockError::Busy)
        ));
        drop(first);
        let second = WindowsFileLock::acquire(&path, root.path()).unwrap();
        drop(second);
        assert_eq!(fs::read(&path).unwrap(), [0]);

        fs::write(&path, [0, 1]).unwrap();
        let existing = WindowsFileLock::acquire(&path, root.path()).unwrap();
        drop(existing);
        assert_eq!(fs::read(path).unwrap(), [0, 1]);
    }
}
