use std::io;
use std::mem::{MaybeUninit, size_of};
use std::os::fd::{FromRawFd, OwnedFd, RawFd};

use super::{NotifyCredentials, NotifyDatagram, NotifySocketReadError};

const MAX_NOTIFY_DATAGRAM: usize = 64 * 1024;
const MAX_RIGHTS_FDS: usize = 64;

pub(super) fn receive_datagram(fd: RawFd) -> Result<NotifyDatagram, NotifySocketReadError> {
    let mut payload = vec![0_u8; MAX_NOTIFY_DATAGRAM];
    let mut control = vec![0_u8; control_buffer_len()];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let mut msg = unsafe { MaybeUninit::<libc::msghdr>::zeroed().assume_init() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = control.len();

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
    let (credentials, fds) = unsafe { parse_control_messages(&msg)? };

    // §1.4's all-or-nothing rule: a datagram that is not delivered whole has no
    // fields applied from it. peinit implements that faithfully against a
    // malformed *line* — the parser fails before anything is applied — and was
    // blind to a truncated one, which is the case it cannot see: the kernel
    // silently cuts an oversized datagram, and a truncated tail can parse as a
    // complete, valid KEY=VALUE line. A long enough STATUS= produced a datagram
    // whose last line was whatever the cut happened to land on, and if the cut
    // fell after an `=` it was well-formed and was applied.
    //
    // MSG_CTRUNC is the same failure for descriptors: beyond the sixty-fourth,
    // they never reach peinit at all, so a service sending seventy with
    // FDSTORE=1 had six vanish with no error and no way to know.
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

unsafe fn parse_control_messages(
    msg: &libc::msghdr,
) -> Result<(Option<NotifyCredentials>, Vec<OwnedFd>), NotifySocketReadError> {
    let mut credentials = None;
    let mut fds = Vec::new();
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(msg) };
    while !cmsg.is_null() {
        let header = unsafe { &*cmsg };
        if header.cmsg_level == libc::SOL_SOCKET && header.cmsg_type == libc::SCM_CREDENTIALS {
            let data = unsafe { libc::CMSG_DATA(cmsg).cast::<libc::ucred>() };
            let cred = unsafe { *data };
            credentials = Some(NotifyCredentials {
                pid: cred.pid as u32,
                uid: cred.uid,
                gid: cred.gid,
            });
        } else if header.cmsg_level == libc::SOL_SOCKET && header.cmsg_type == libc::SCM_RIGHTS {
            let data = unsafe { libc::CMSG_DATA(cmsg).cast::<RawFd>() };
            let byte_len = header
                .cmsg_len
                .saturating_sub(align_to_usize(size_of::<libc::cmsghdr>()));
            let fd_count = byte_len / size_of::<RawFd>();
            for index in 0..fd_count {
                let fd = unsafe { *data.add(index) };
                if fd >= 0 {
                    fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
                }
            }
        }
        cmsg = unsafe { libc::CMSG_NXTHDR(msg, cmsg) };
    }
    Ok((credentials, fds))
}
