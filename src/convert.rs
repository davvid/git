/// Rust equivalent to struct checkout_metadata from convert.h.
#[repr(C)]
pub struct CheckoutMetadata {
    refname: *const std::ffi::c_char,
    treeish: crate::hash::ObjectID,
    blob: crate::hash::ObjectID,
}

impl Default for CheckoutMetadata {
    fn default() -> Self {
        Self {
            refname: std::ptr::null(),
            treeish: crate::hash::HashAlgorithm::SHA256.null_oid().clone(),
            blob: crate::hash::HashAlgorithm::SHA256.null_oid().clone(),
        }
    }
}
