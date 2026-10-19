//! C ABI implementation for the public headers in `include/peinit/`.
//!
//! # Pointer and handle contracts
//!
//! Non-null input strings must remain readable through their terminating NUL
//! for the entire call. Output pointers must be aligned, writable, and must not
//! alias inputs or other outputs. A null pointer is accepted only where the
//! matching header documents it; rejecting null does not validate other pointers.
//!
//! Handles come from this library and remain live until their matching free
//! function consumes them. Mutating calls, including client requests and freeing,
//! require exclusive access to the handle. Response/error string accessors borrow
//! storage until the owner is freed; callers must neither mutate nor free it.
//! Descriptor arguments remain open and owned by the caller throughout a call.
//! Internal pointer helpers rely on these same entry-point contracts.

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

/// Return the C ABI generation independently of the implementation version.
#[unsafe(no_mangle)]
pub extern "C" fn peinit_abi_version() -> c_uint {
    ABI_VERSION
}

/// Return a static, NUL-terminated implementation version from build metadata.
#[unsafe(no_mangle)]
pub extern "C" fn peinit_library_version() -> *const libc::c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

#[cfg(test)]
mod tests;
