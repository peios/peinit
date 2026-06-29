use std::io;
#[cfg(test)]
use std::os::fd::{AsRawFd, OwnedFd};
#[cfg(test)]
use std::time::{Duration, Instant};

use super::super::model::{ChildSetupEvidence, ChildSetupStatus, MalformedChildSetupEvidence};

#[cfg(test)]
pub(in crate::boundary::linux_launch::process) fn read_child_setup_status(
    fd: &OwnedFd,
    setup_timeout_secs: u64,
) -> io::Result<ChildSetupStatus> {
    let deadline = child_setup_deadline(setup_timeout_secs)?;
    let mut bytes = [0u8; ChildSetupEvidence::BYTE_LEN + 1];
    let mut offset = 0;
    loop {
        let read = unsafe {
            libc::read(
                fd.as_raw_fd(),
                bytes[offset..].as_mut_ptr().cast(),
                bytes.len() - offset,
            )
        };
        if read == 0 {
            if offset == 0 {
                return Ok(ChildSetupStatus::ExecSucceeded);
            }
            break;
        }
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                wait_for_child_setup_pipe(fd.as_raw_fd(), deadline)?;
                continue;
            }
            return Err(error);
        }
        offset += read as usize;
        if offset == bytes.len() {
            break;
        }
    }
    if offset != ChildSetupEvidence::BYTE_LEN {
        return Ok(ChildSetupStatus::MalformedSetupEvidence(
            MalformedChildSetupEvidence::InvalidLength { len: offset },
        ));
    }
    let mut payload = [0u8; ChildSetupEvidence::BYTE_LEN];
    payload.copy_from_slice(&bytes[..ChildSetupEvidence::BYTE_LEN]);
    Ok(match ChildSetupEvidence::decode(payload) {
        Ok(evidence) => ChildSetupStatus::SetupFailed(evidence),
        Err(error) => ChildSetupStatus::MalformedSetupEvidence(error),
    })
}

pub(in crate::boundary::linux_launch::process) fn read_child_setup_status_nonblocking(
    fd: i32,
) -> io::Result<ChildSetupStatus> {
    if fd < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "child setup status fd is invalid",
        ));
    }
    let mut bytes = [0u8; ChildSetupEvidence::BYTE_LEN + 1];
    let read = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
    if read == 0 {
        return Ok(ChildSetupStatus::ExecSucceeded);
    }
    if read < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted || error.kind() == io::ErrorKind::WouldBlock {
            return Ok(ChildSetupStatus::Pending);
        }
        return Err(error);
    }
    let len = read as usize;
    if len != ChildSetupEvidence::BYTE_LEN {
        return Ok(ChildSetupStatus::MalformedSetupEvidence(
            MalformedChildSetupEvidence::InvalidLength { len },
        ));
    }
    let mut payload = [0u8; ChildSetupEvidence::BYTE_LEN];
    payload.copy_from_slice(&bytes[..ChildSetupEvidence::BYTE_LEN]);
    Ok(match ChildSetupEvidence::decode(payload) {
        Ok(evidence) => ChildSetupStatus::SetupFailed(evidence),
        Err(error) => ChildSetupStatus::MalformedSetupEvidence(error),
    })
}

#[cfg(test)]
fn child_setup_deadline(setup_timeout_secs: u64) -> io::Result<Instant> {
    Instant::now()
        .checked_add(Duration::from_secs(setup_timeout_secs))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "setup timeout too large"))
}

#[cfg(test)]
fn wait_for_child_setup_pipe(fd: i32, deadline: Instant) -> io::Result<()> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "child setup status timed out",
            ));
        }
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut pollfd, 1, poll_timeout_ms(deadline - now)) };
        if result > 0 {
            if pollfd.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "child setup status fd is invalid",
                ));
            }
            return Ok(());
        }
        if result == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "child setup status timed out",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(test)]
fn poll_timeout_ms(remaining: Duration) -> libc::c_int {
    let millis = remaining.as_millis();
    if millis == 0 && !remaining.is_zero() {
        1
    } else {
        millis.min(libc::c_int::MAX as u128) as libc::c_int
    }
}
