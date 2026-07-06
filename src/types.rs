/// Exit codes used by builtins.
pub enum ExitCode {
    /// Command succeeded
    Success = 0,
    // Command failed
    Failure = 1,
    /// General error
    Error = 128,
    /// Usage or unknown option error
    UsageError = 129,
}

/// Convert a C string pointer into an owned OsString.
///
/// # Safety
/// cstr_ptr is not null and points to a null-terminated C string.
pub unsafe fn os_string_from_c_char_ptr(cstr_ptr: *const std::ffi::c_char) -> std::ffi::OsString {
    let cstr = std::ffi::CStr::from_ptr(cstr_ptr);

    std::ffi::OsString::from(cstr.to_str().unwrap_or_default())
}

/// Converts a null encoded C string pointer into an Option<OsString>.
/// The None variant is returned when the pointer is null.
///
/// # Safety
/// cstr_ptr is not null.
pub unsafe fn maybe_os_string_from_c_char_ptr(
    cstr_ptr: *const std::ffi::c_char,
) -> Option<std::ffi::OsString> {
    if cstr_ptr.is_null() {
        None
    } else {
        Some(os_string_from_c_char_ptr(cstr_ptr))
    }
}

/// Convert a C string pointer into a str slice
///
/// # Safety
/// cstr_ptr points to a null-terminated C string.
pub unsafe fn maybe_str_from_c_char_ptr<'a>(cstr_ptr: *const std::ffi::c_char) -> Option<&'a str> {
    if cstr_ptr.is_null() {
        None
    } else {
        std::ffi::CStr::from_ptr(cstr_ptr).to_str().ok()
    }
}

/// Convert a C string pointer into an owned PathBuf.
///
/// # Safety
/// cstr_ptr points to a null-terminated C string.
pub unsafe fn pathbuf_from_c_char_ptr(cstr_ptr: *const std::ffi::c_char) -> std::path::PathBuf {
    std::path::PathBuf::from(os_string_from_c_char_ptr(cstr_ptr))
}
