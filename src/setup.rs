pub mod c {
    extern "C" {
        pub fn git_get_startup_info() -> *const std::ffi::c_void;
        pub fn git_startup_info_have_repository(info: *const std::ffi::c_void) -> i32;
        pub fn setup_work_tree(repo: *mut std::ffi::c_void);
    }
}
