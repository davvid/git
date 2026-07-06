use crate::hash::ObjectID;

/// Calculate and return a hex object ID from an ObjectID struct.
pub fn hex_from_oid(oid: &ObjectID) -> &str {
    let oid_ptr = std::ptr::addr_of!(*oid);

    // Safety: oid_ptr is not NULL.
    let cptr = unsafe { c::oid_to_hex(oid_ptr) };
    // Safety: cptr is not NULL and NULL-terminated.
    let cstr = unsafe { std::ffi::CStr::from_ptr(cptr) };

    cstr.to_str().expect(&format!("invalid UTF-8: {oid:?}"))
}


pub mod c {
    use super::*;

    extern "C" {
        pub fn get_oid_hex_algop(
            hex: *const std::ffi::c_char,
            oid: *mut ObjectID,
            git_hash_algo: *const std::ffi::c_void,
        ) -> i32;

        pub fn oid_to_hex(oid: *const ObjectID) -> *const std::ffi::c_char;
    }
}
