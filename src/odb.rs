/// # Safety
///
/// Pointers are not null.
pub unsafe fn odb_read_object_into_utf8_string(
    odb: *mut std::ffi::c_void,
    oid: &crate::hash::ObjectID,
    object_type: &mut crate::object::ObjectType,
) -> Option<String>
{
    let oid = std::ptr::addr_of!(*oid);
    let object_type_ptr = std::ptr::addr_of_mut!(*object_type);
    let mut size: usize = 0;
    let size_ptr = std::ptr::addr_of_mut!(size);
    // Safety: pointers are not null.
    let void_ptr = unsafe { c::odb_read_object(odb, oid, object_type_ptr, size_ptr) };
    if void_ptr.is_null() {
        return None;
    }
    // Safety: void_ptr points to C-allocated memory.
    let bytes = unsafe { std::slice::from_raw_parts(void_ptr as *const u8, *size_ptr) };
    let bytes_vec: Vec<u8> = bytes.to_vec(); // Copy data from C to Rust.

    // Safety: void_ptr was allocated by C and is no longer accessed.
    unsafe { libc::free(void_ptr) };

    String::from_utf8(bytes_vec).ok()
}

pub mod c {
    extern "C" {
        pub fn odb_read_object(
            odb: *mut std::ffi::c_void,
            oid: *const crate::hash::ObjectID,
            object_type: *mut crate::object::ObjectType,
            size: *mut usize,
        ) -> *mut std::ffi::c_void;
    }
}
