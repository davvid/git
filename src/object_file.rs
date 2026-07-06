pub mod c {
    extern "C" {
        pub fn index_fd(
            istate: *mut std::ffi::c_void,
            oid: *mut crate::hash::ObjectID,
            fd: i32,
            st: *mut libc::stat,
            object_type: i32,
            path: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
}
