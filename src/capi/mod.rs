mod control;
mod error;
mod jobs;
mod notify;
mod types;

pub use control::*;
pub use error::*;
pub use jobs::*;
pub use notify::*;
pub use types::*;

use libc::c_uint;

const ABI_VERSION: c_uint = 0;

#[unsafe(no_mangle)]
pub extern "C" fn peinit_abi_version() -> c_uint {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn peinit_library_version() -> *const libc::c_char {
    c"0.0.1".as_ptr()
}

#[cfg(test)]
mod tests;
