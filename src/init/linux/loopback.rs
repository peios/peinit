use std::ffi::CString;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

const LOOPBACK_INTERFACE: &str = "lo";
const NETLINK_ROUTE: i32 = 0;
const NLM_F_REQUEST: u16 = 0x01;
const NLM_F_ACK: u16 = 0x04;
const NLMSG_ERROR: u16 = 0x02;
const RTM_NEWLINK: u16 = 16;

pub(super) fn bring_up_loopback<N>(netlink: &mut N) -> io::Result<()>
where
    N: LoopbackNetlink + ?Sized,
{
    netlink.bring_up_loopback()
}

pub(super) trait LoopbackNetlink {
    fn bring_up_loopback(&mut self) -> io::Result<()>;
}

pub(super) struct LinuxLoopbackNetlink;

impl LoopbackNetlink for LinuxLoopbackNetlink {
    fn bring_up_loopback(&mut self) -> io::Result<()> {
        bring_up_loopback_with_netlink()
    }
}

fn bring_up_loopback_with_netlink() -> io::Result<()> {
    let ifindex = loopback_ifindex()?;
    let socket = open_route_netlink_socket()?;
    let request = newlink_up_request(ifindex, 1);
    send_netlink_request(socket.as_raw_fd(), &request)?;
    receive_netlink_ack(socket.as_raw_fd(), 1)
}

fn loopback_ifindex() -> io::Result<i32> {
    let name = CString::new(LOOPBACK_INTERFACE)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
    if index == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(index as i32)
    }
}

fn open_route_netlink_socket() -> io::Result<OwnedFd> {
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            NETLINK_ROUTE,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut address = netlink_address(0);
    let rc = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            (&mut address as *mut libc::sockaddr_nl).cast::<libc::sockaddr>(),
            size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

fn newlink_up_request(ifindex: i32, sequence: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size_of::<libc::nlmsghdr>() + size_of::<libc::ifinfomsg>());
    let header = libc::nlmsghdr {
        nlmsg_len: (size_of::<libc::nlmsghdr>() + size_of::<libc::ifinfomsg>()) as u32,
        nlmsg_type: RTM_NEWLINK,
        nlmsg_flags: NLM_F_REQUEST | NLM_F_ACK,
        nlmsg_seq: sequence,
        nlmsg_pid: 0,
    };
    let mut info = unsafe { zeroed::<libc::ifinfomsg>() };
    info.ifi_family = libc::AF_UNSPEC as u8;
    info.ifi_index = ifindex;
    info.ifi_flags = libc::IFF_UP as u32;
    info.ifi_change = libc::IFF_UP as u32;
    append_struct(&mut bytes, &header);
    append_struct(&mut bytes, &info);
    bytes
}

fn send_netlink_request(fd: i32, request: &[u8]) -> io::Result<()> {
    let mut kernel = netlink_address(0);
    let sent = unsafe {
        libc::sendto(
            fd,
            request.as_ptr().cast(),
            request.len(),
            0,
            (&mut kernel as *mut libc::sockaddr_nl).cast::<libc::sockaddr>(),
            size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if sent < 0 {
        return Err(io::Error::last_os_error());
    }
    if sent as usize != request.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!("short netlink send: {sent} of {}", request.len()),
        ));
    }
    Ok(())
}

fn receive_netlink_ack(fd: i32, sequence: u32) -> io::Result<()> {
    let mut buffer = [0_u8; 8192];
    loop {
        let received = unsafe { libc::recv(fd, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
        if received < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if received == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "netlink socket closed before ACK",
            ));
        }
        return parse_netlink_ack(&buffer[..received as usize], sequence);
    }
}

fn parse_netlink_ack(bytes: &[u8], sequence: u32) -> io::Result<()> {
    if bytes.len() < size_of::<libc::nlmsghdr>() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short netlink ACK header",
        ));
    }
    let header = read_unaligned::<libc::nlmsghdr>(bytes);
    if header.nlmsg_seq != sequence {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected netlink ACK sequence {}", header.nlmsg_seq),
        ));
    }
    if header.nlmsg_type != NLMSG_ERROR {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected netlink ACK type {}", header.nlmsg_type),
        ));
    }
    let error_offset = nlmsg_align(size_of::<libc::nlmsghdr>());
    if bytes.len() < error_offset + size_of::<libc::nlmsgerr>() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short netlink ACK payload",
        ));
    }
    let ack = read_unaligned::<libc::nlmsgerr>(&bytes[error_offset..]);
    if ack.error == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(-ack.error))
    }
}

fn netlink_address(pid: u32) -> libc::sockaddr_nl {
    let mut address = unsafe { zeroed::<libc::sockaddr_nl>() };
    address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    address.nl_pid = pid;
    address
}

fn append_struct<T>(bytes: &mut Vec<u8>, value: &T) {
    let value_bytes =
        unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) };
    bytes.extend_from_slice(value_bytes);
}

fn read_unaligned<T: Copy>(bytes: &[u8]) -> T {
    unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) }
}

const fn nlmsg_align(len: usize) -> usize {
    (len + 3) & !3
}
