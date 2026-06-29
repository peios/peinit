use std::io::{self, ErrorKind};
use std::mem::{MaybeUninit, size_of};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{NotifySocket, NotifySocketBindError};

#[test]
fn receives_real_unix_datagram_with_kernel_credentials() {
    let path = temp_socket_path("peinit2-notify");
    let notify = match NotifySocket::bind(&path) {
        Ok(socket) => socket,
        Err(NotifySocketBindError::Bind { source, .. })
            if source.kind() == ErrorKind::PermissionDenied =>
        {
            return;
        }
        Err(error) => panic!("bind notify socket: {error:?}"),
    };
    let sender = UnixDatagram::unbound().expect("sender socket");

    sender
        .send_to(b"READY=1\nSTATUS=ok", notify.path())
        .expect("send datagram");
    let datagram = notify.receive().expect("receive datagram");

    assert_eq!(datagram.payload, b"READY=1\nSTATUS=ok");
    assert_eq!(datagram.credentials.pid, std::process::id());
    assert_eq!(datagram.fds.len(), 0);
    assert!(sender.as_raw_fd() >= 0);
}

#[test]
fn received_rights_are_close_on_exec() {
    let path = temp_socket_path("peinit2-notify-rights");
    let notify = match NotifySocket::bind(&path) {
        Ok(socket) => socket,
        Err(NotifySocketBindError::Bind { source, .. })
            if source.kind() == ErrorKind::PermissionDenied =>
        {
            return;
        }
        Err(error) => panic!("bind notify socket: {error:?}"),
    };
    let sender = UnixDatagram::unbound().expect("sender socket");
    sender.connect(notify.path()).expect("connect sender");
    let (sent_fd, _peer) = UnixStream::pair().expect("fd pair");

    send_datagram_with_fd(sender.as_raw_fd(), b"FDSTORE=1", sent_fd.as_raw_fd())
        .expect("send fd datagram");
    let datagram = notify.receive().expect("receive datagram");

    assert_eq!(datagram.payload, b"FDSTORE=1");
    assert_eq!(datagram.fds.len(), 1);
    assert!(fd_has_cloexec(datagram.fds[0].as_raw_fd()));
}

fn temp_socket_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.sock", std::process::id()))
}

fn send_datagram_with_fd(socket_fd: RawFd, payload: &[u8], fd: RawFd) -> io::Result<()> {
    let mut control = vec![0_u8; cmsg_space(size_of::<RawFd>())];
    let mut iov = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    let mut msg = unsafe { MaybeUninit::<libc::msghdr>::zeroed().assume_init() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = control.len();

    let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    if cmsg.is_null() {
        return Err(io::Error::other("missing control header"));
    }
    unsafe {
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = cmsg_len(size_of::<RawFd>());
        let data = libc::CMSG_DATA(cmsg).cast::<RawFd>();
        *data = fd;
    }

    if unsafe { libc::sendmsg(socket_fd, &msg, 0) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn fd_has_cloexec(fd: RawFd) -> bool {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    assert!(flags >= 0, "fcntl(F_GETFD) failed");
    flags & libc::FD_CLOEXEC != 0
}

fn cmsg_space(payload_len: usize) -> usize {
    align_to_usize(size_of::<libc::cmsghdr>()) + align_to_usize(payload_len)
}

fn cmsg_len(payload_len: usize) -> usize {
    align_to_usize(size_of::<libc::cmsghdr>()) + payload_len
}

fn align_to_usize(value: usize) -> usize {
    let align = size_of::<usize>();
    (value + align - 1) & !(align - 1)
}
