use std::ffi::CString;
use std::io;

use crate::service::{ServiceCheck, ServiceCheckKind};

pub(super) fn run_child_checks(checks: &[ServiceCheck], write_fd: i32) -> ! {
    let mut payload = Vec::with_capacity(4 + checks.len());
    payload.extend_from_slice(&(checks.len() as u32).to_le_bytes());
    payload.extend(checks.iter().map(check_satisfied).map(u8::from));
    let ok = write_all(write_fd, &payload).is_ok();
    unsafe { libc::_exit(if ok { 0 } else { 1 }) }
}

fn check_satisfied(check: &ServiceCheck) -> bool {
    let Ok(path) = CString::new(check.argument.as_str()) else {
        return false;
    };
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let rc = unsafe { libc::stat(path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return false;
    }
    let stat = unsafe { stat.assume_init() };
    match check.kind {
        ServiceCheckKind::Path => true,
        ServiceCheckKind::File => (stat.st_mode & libc::S_IFMT) == libc::S_IFREG,
        ServiceCheckKind::Directory => (stat.st_mode & libc::S_IFMT) == libc::S_IFDIR,
        ServiceCheckKind::Registry => false,
    }
}

fn write_all(fd: i32, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written > 0 {
            bytes = &bytes[written as usize..];
            continue;
        }
        if written == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0"));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Ok(())
}
