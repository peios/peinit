mod model;
#[cfg(any(test, feature = "peios-boundary"))]
mod syscall;

#[cfg(feature = "peios-boundary")]
mod devices;

#[cfg(feature = "peios-boundary")]
pub use devices::LinuxPowerButtonDevices;
pub use model::{LinuxPowerButtonRead, LinuxPowerButtonReadError};

#[cfg(feature = "peios-boundary")]
fn set_cloexec_nonblocking(fd: i32) -> std::io::Result<()> {
    let fd_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if fd_flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, fd_flags | libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let status_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if status_flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, status_flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
