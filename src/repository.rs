pub mod c {
    use std::ffi::{c_char, c_void};

    use crate::config::c::ConfigFn;

    extern "C" {
        pub fn repo_config(repo: *mut c_void, func: *const ConfigFn, data: *mut c_void) -> i32;
        pub fn repo_get_git_dir(repo: *mut c_void) -> *const c_char;
        pub fn repo_get_hash_algo(repo: *const c_void) -> *const c_void;
        pub fn repo_get_index_state(repo: *const c_void) -> *mut c_void;
        pub fn repo_get_work_tree(repo: *mut c_void) -> *const c_char;
        pub fn repo_get_object_database(repo: *mut c_void) -> *mut c_void;
    }
}
