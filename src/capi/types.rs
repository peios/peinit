#![allow(non_camel_case_types)]

use libc::{c_char, c_int, c_void};

pub type peinit_client_t = c_void;
pub type peinit_response_t = c_void;
pub type peinit_error_t = c_void;

pub const PEINIT_OK: c_int = 0;
pub const PEINIT_ERR_INVALID_ARGUMENT: c_int = -1;
pub const PEINIT_ERR_IO: c_int = -2;
pub const PEINIT_ERR_PROTOCOL: c_int = -3;
pub const PEINIT_ERR_UNAVAILABLE: c_int = -4;
pub const PEINIT_ERR_NO_MEMORY: c_int = -5;
pub const PEINIT_ERR_INTERNAL: c_int = -6;

pub const PEINIT_SHUTDOWN_POWEROFF: c_int = 0;
pub const PEINIT_SHUTDOWN_REBOOT: c_int = 1;
pub const PEINIT_SHUTDOWN_HALT: c_int = 2;

#[unsafe(no_mangle)]
pub extern "C" fn peinit_status_name(status: c_int) -> *const c_char {
    match status {
        PEINIT_OK => c"ok".as_ptr(),
        PEINIT_ERR_INVALID_ARGUMENT => c"invalid_argument".as_ptr(),
        PEINIT_ERR_IO => c"io".as_ptr(),
        PEINIT_ERR_PROTOCOL => c"protocol".as_ptr(),
        PEINIT_ERR_UNAVAILABLE => c"unavailable".as_ptr(),
        PEINIT_ERR_NO_MEMORY => c"no_memory".as_ptr(),
        PEINIT_ERR_INTERNAL => c"internal".as_ptr(),
        _ => c"unknown".as_ptr(),
    }
}
