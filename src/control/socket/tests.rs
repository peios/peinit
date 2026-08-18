use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::address::unix_socket_address;
use super::{
    CONTROL_SOCKET_PATH, ControlSocketPathError, ControlSocketRead, ControlSocketWrite,
    DEFAULT_CONNECTION_TIMEOUT_SECS, DEFAULT_MAX_CONTROL_CONNECTIONS,
    DEFAULT_MAX_REQUEST_SIZE_BYTES, LinuxControlSocket,
};

#[test]
fn control_socket_constants_match_psd_007() {
    assert_eq!(CONTROL_SOCKET_PATH, "/run/services/peinit/control.sock");
    assert_eq!(DEFAULT_MAX_CONTROL_CONNECTIONS, 32);
    assert_eq!(DEFAULT_MAX_REQUEST_SIZE_BYTES, 65_536);
    assert_eq!(DEFAULT_CONNECTION_TIMEOUT_SECS, 30);
}

#[test]
fn unix_socket_address_rejects_invalid_paths() {
    assert_eq!(
        unix_socket_address(std::path::Path::new("")).expect_err("empty"),
        ControlSocketPathError::Empty,
    );
    assert_eq!(
        unix_socket_address(std::path::Path::new("abc\0def")).expect_err("nul"),
        ControlSocketPathError::InteriorNul { index: 3 },
    );
    let too_long = "a".repeat(108);
    assert_eq!(
        unix_socket_address(std::path::Path::new(&too_long)).expect_err("too long"),
        ControlSocketPathError::TooLong { len: 108, max: 107 },
    );
}

#[test]
fn linux_control_socket_accepts_reads_and_writes() {
    let path = temp_socket_path("peinit2-control");
    let listener = match LinuxControlSocket::bind(&path) {
        Ok(socket) => socket,
        Err(super::ControlSocketBindError::Bind { source, .. })
            if source.kind() == ErrorKind::PermissionDenied =>
        {
            return;
        }
        Err(error) => panic!("bind control socket: {error:?}"),
    };
    let mut client = UnixStream::connect(listener.path()).expect("connect client");
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("read timeout");
    client
        .write_all(b"{\"command\":\"shutdown\",\"type\":\"reboot\"}\n")
        .expect("client write");

    let connection = listener
        .accept()
        .expect("accept")
        .expect("accepted connection");
    assert!(connection.as_raw_fd() >= 0);
    assert_eq!(
        connection.read(128).expect("server read"),
        ControlSocketRead::Bytes(b"{\"command\":\"shutdown\",\"type\":\"reboot\"}\n".to_vec()),
    );
    assert_eq!(
        connection
            .write(b"{\"status\":\"ok\"}\n")
            .expect("server write"),
        ControlSocketWrite::Complete,
    );
    drop(connection);

    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("client read response");
    assert_eq!(response, "{\"status\":\"ok\"}\n");
    assert!(listener.as_raw_fd() >= 0);
}

fn temp_socket_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.sock", std::process::id()))
}
