pub const COMMIT_LOCK: u32 = 1 << 0;

pub struct IndexState {
    ptr: *mut std::ffi::c_void,
}
impl IndexState {
    /// # Safety
    /// repo is not NULL.
    pub unsafe fn new(repo: *mut std::ffi::c_void) -> Self {
        Self { ptr: c::index_state_create(repo) }
    }

    pub fn as_ptr(&self) -> *const std::ffi::c_void {
        self.ptr
    }

    pub fn as_mut_ptr(&self) -> *mut std::ffi::c_void {
        self.ptr
    }
}
impl Drop for IndexState {
    fn drop(&mut self) {
        // Safety: ptr was created by index_state_create and is valid.
        unsafe { c::index_state_free(self.ptr) };
    }
}

// Rust equivalent of the `ADD_CACHE_*` definitions from read-cache-ll.h.
#[repr(C)]
pub enum AddCache {
    OkToAdd = 1,        // Ok to add
    OkToReplace = 2,    // Ok to replace file/directory
    SkipDFCheck = 4,    // Ok to skip DF conflict checks
    JustAppend = 8,     // Append only
    NewOnly = 16,       // Do not replace existing ones
    KeepCacheTree = 32, // Do not invalidate cache-tree
    Renormalize = 64,   // Pass along HASH_RENORMALIZE
}

pub mod c {
    extern "C" {
        pub fn add_index_entry(
            istate: *mut std::ffi::c_void,
            ce: *mut std::ffi::c_void,
            option: i32,
        ) -> i32;

        pub fn discard_cache_entry(ce: *mut std::ffi::c_void);

        pub fn index_state_create(repo: *mut std::ffi::c_void) -> *mut std::ffi::c_void;

        pub fn index_state_free(index_state: *mut std::ffi::c_void);

        pub fn index_state_get_cache_nr(index_state: *const std::ffi::c_void) -> u32;

        pub fn index_state_get_cache_entry_name(
            index_state: *const std::ffi::c_void,
            idx: usize,
        ) -> *const std::ffi::c_char;

        pub fn make_cache_entry(
            istate: *mut std::ffi::c_void,
            mode: u32,
            oid: *const crate::hash::ObjectID,
            path: *const std::ffi::c_char,
            stage: i32,
            refresh_options: i32,
        ) -> *mut std::ffi::c_void;

        pub fn make_transient_cache_entry(
            mode: u32,
            oid: *const crate::hash::ObjectID,
            path: *const std::ffi::c_char,
            stage: i32,
            ce_mem_pool: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;

        pub fn write_locked_index(
            index_state: *mut std::ffi::c_void,
            lock: *mut crate::lockfile::LockFile,
            flags: u32,
        ) -> i32;
    }
}
