pub mod c {
    use std::ffi::{c_char, c_void};

    pub type ConfigFn = extern "C" fn(
        key: *const c_char,
        value: *const c_char,
        context: *const c_void,
        data: *mut c_void,
    ) -> i32;

    extern "C" {
        pub fn git_config_bool(key: *const c_char, value: *const c_char) -> bool;
        pub fn git_default_config(
            var: *const c_char,
            value: *const c_char,
            ctx: *const c_void,
            cb: *mut c_void,
        ) -> i32;
    }
}
