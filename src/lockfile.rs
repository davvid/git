/// Rust equivalent to struct lock_file from lockfile.h.
#[repr(C)]
pub struct LockFile {
    tempfile: *mut std::ffi::c_void,
}

impl LockFile {
    pub fn as_mut_ptr(&mut self) -> *mut LockFile {
        std::ptr::addr_of_mut!(*self)
    }
}

impl Default for LockFile {
    fn default() -> Self {
        Self {
            tempfile: std::ptr::null_mut(),
        }
    }
}

pub fn hold_lock_file_for_update(lock: &mut LockFile, path: &str, flags: i32) -> i32
{
    let Ok(cstring) = std::ffi::CString::new(path) else {
        return 1;
    };

    // Safety: pointers are not NULL.
    unsafe {
        c::hold_lock_file_for_update_timeout_mode(
            std::ptr::addr_of_mut!(*lock),
            cstring.as_ptr(),
            flags,
            0,
            0o666,
        )
    }
}

pub mod c {
    extern "C" {
        pub fn hold_lock_file_for_update_timeout_mode(
            lock: *mut super::LockFile,
            path: *const std::ffi::c_char,
            flags: i32,
            timeout_mfs: isize,
            mode: i32,
        ) -> i32;
    }
}
