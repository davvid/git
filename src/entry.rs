use bitfield_struct::bitfield;

/// Rust equivalent to struct checkout from entry.h.
#[repr(C)]
pub struct Checkout {
    pub istate: *mut std::ffi::c_void, // struct index_state*
    pub base_dir: *const std::ffi::c_char,
    pub base_dir_len: i32,
    pub super_prefix: *const std::ffi::c_char,
    pub delayed_checkout: *mut std::ffi::c_void, // struct delayed_checkout*
    pub meta: crate::convert::CheckoutMetadata,
    pub fields: CheckoutBitFields,
}

impl Default for Checkout {
    fn default() -> Self {
        Self {
            istate: std::ptr::null_mut(),
            base_dir: std::ptr::null(),
            base_dir_len: 0,
            super_prefix: std::ptr::null(),
            delayed_checkout: std::ptr::null_mut(),
            meta: crate::convert::CheckoutMetadata::default(),
            fields: CheckoutBitFields::default(),
        }
    }
}


#[bitfield(u32)]
pub struct CheckoutBitFields {
    pub force: bool,
    pub quiet: bool,
    pub not_new: bool,
    pub clone: bool,
    pub refresh_cache: bool,
    #[bits(27)]
    _padding: u32,
}

pub mod c {
    use std::ffi::{c_char, c_void};
    use super::Checkout;

    extern "C" {
        pub fn checkout_entry_ca(
            ce: *mut c_void,
            ca: *mut c_void,
            state: *mut Checkout,
            topath: *mut c_char,
            nr_checkouts: *mut i32,
        ) -> i32;
    }
}
