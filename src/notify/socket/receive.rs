use std::io;
use std::mem::{MaybeUninit, align_of, size_of, size_of_val};
use std::os::fd::{FromRawFd, OwnedFd, RawFd};

use super::{NotifyCredentials, NotifyDatagram, NotifySocketReadError};

const MAX_NOTIFY_DATAGRAM: usize = 64 * 1024;
const MAX_RIGHTS_FDS: usize = 64;

pub(super) fn receive_datagram(fd: RawFd) -> Result<NotifyDatagram, NotifySocketReadError> {
    let mut payload = vec![0_u8; MAX_NOTIFY_DATAGRAM];
    let mut control = control_buffer();
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    // SAFETY: an all-zero msghdr is valid before its buffer pointers are set.
    let mut msg = unsafe { MaybeUninit::<libc::msghdr>::zeroed().assume_init() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = size_of_val(control.as_slice());

    // SAFETY: all writable buffers remain live and uniquely borrowed for recvmsg.
    let len = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if len == -1 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            return Err(NotifySocketReadError::WouldBlock);
        }
        return Err(NotifySocketReadError::Recv(error));
    }
    payload.truncate(len as usize);

    // Parse before the truncation check, so any descriptors that *did* arrive
    // are owned and closed on the way out rather than leaked.
    // SAFETY: recvmsg filled this aligned buffer with kernel-generated control records.
    let (credentials, fds) = unsafe { parse_control_messages(&msg)? };

    // Reject either form of truncation before applying any notification fields.
    // Parsing first gives ownership to every descriptor that reached this process.
    let payload_truncated = msg.msg_flags & libc::MSG_TRUNC != 0;
    let control_truncated = msg.msg_flags & libc::MSG_CTRUNC != 0;
    if payload_truncated || control_truncated {
        // `fds` drops here, closing whatever arrived.
        return Err(NotifySocketReadError::Truncated {
            payload: payload_truncated,
            control: control_truncated,
        });
    }

    let credentials = credentials.ok_or(NotifySocketReadError::MissingCredentials)?;
    Ok(NotifyDatagram {
        payload,
        credentials,
        fds,
    })
}

// Linux control headers and their credential/descriptor payloads fit word alignment.
const _: () = assert!(align_of::<usize>() >= align_of::<libc::cmsghdr>());
const _: () = assert!(align_of::<usize>() >= align_of::<libc::ucred>());
const _: () = assert!(align_of::<usize>() >= align_of::<RawFd>());

fn control_buffer() -> Vec<usize> {
    vec![0; control_buffer_len().div_ceil(size_of::<usize>())]
}

fn control_buffer_len() -> usize {
    cmsg_space(size_of::<libc::ucred>())
        + cmsg_space(MAX_RIGHTS_FDS.saturating_mul(size_of::<RawFd>()))
}

fn cmsg_space(payload_len: usize) -> usize {
    align_to_usize(size_of::<libc::cmsghdr>()) + align_to_usize(payload_len)
}

fn align_to_usize(value: usize) -> usize {
    let align = size_of::<usize>();
    (value + align - 1) & !(align - 1)
}

/// Take ownership of all received descriptors before validating the datagram.
///
/// # Safety
/// `msg` must describe the live, aligned control buffer filled by a successful
/// Linux recvmsg call. The caller must not have taken ownership of its descriptors.
unsafe fn parse_control_messages(
    msg: &libc::msghdr,
) -> Result<(Option<NotifyCredentials>, Vec<OwnedFd>), NotifySocketReadError> {
    let mut credentials = None;
    let mut fds = Vec::new();
    // SAFETY: the caller supplies a live recvmsg buffer with kernel record lengths.
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(msg) };
    while !cmsg.is_null() {
        // SAFETY: CMSG traversal returned a complete, aligned header in the buffer.
        let header = unsafe { &*cmsg };
        if header.cmsg_level == libc::SOL_SOCKET && header.cmsg_type == libc::SCM_CREDENTIALS {
            // SAFETY: Linux supplies a complete ucred for SCM_CREDENTIALS.
            let data = unsafe { libc::CMSG_DATA(cmsg).cast::<libc::ucred>() };
            // SAFETY: the credential payload is aligned and initialized by recvmsg.
            let cred = unsafe { *data };
            credentials = Some(NotifyCredentials {
                pid: cred.pid as u32,
                uid: cred.uid,
                gid: cred.gid,
            });
        } else if header.cmsg_level == libc::SOL_SOCKET && header.cmsg_type == libc::SCM_RIGHTS {
            // SAFETY: Linux supplies an aligned array of installed descriptors.
            let data = unsafe { libc::CMSG_DATA(cmsg).cast::<RawFd>() };
            let byte_len = header
                .cmsg_len
                .saturating_sub(align_to_usize(size_of::<libc::cmsghdr>()));
            let fd_count = byte_len / size_of::<RawFd>();
            for index in 0..fd_count {
                // SAFETY: the index is bounded by the kernel-provided payload length.
                let fd = unsafe { *data.add(index) };
                if fd >= 0 {
                    // SAFETY: each received descriptor is newly owned by this process.
                    fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
                }
            }
        }
        // SAFETY: both pointers describe the same live kernel control buffer.
        cmsg = unsafe { libc::CMSG_NXTHDR(msg, cmsg) };
    }
    Ok((credentials, fds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_storage_guarantees_header_and_payload_alignment() {
        let storage = control_buffer();
        assert!(align_of::<usize>() >= align_of::<libc::cmsghdr>());
        assert_eq!(storage.as_ptr() as usize % align_of::<libc::cmsghdr>(), 0);
        assert!(size_of_val(storage.as_slice()) >= control_buffer_len());
    }
}
