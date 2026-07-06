pub mod c {
    extern "C" {
        pub fn ensure_full_index(istate: *mut std::ffi::c_void);
    }
}
