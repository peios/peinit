use std::os::unix::net::UnixDatagram;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::logging::{LogStream, ServiceLogRecord};

use super::send_eventd_log_record;

static SOCKET_ID: AtomicU64 = AtomicU64::new(0);

#[test]
fn eventd_log_sender_writes_msgpack_datagram_to_unix_socket() {
    let path = std::env::temp_dir().join(format!(
        "peinit2-eventd-log-{}-{}.sock",
        std::process::id(),
        SOCKET_ID.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::remove_file(&path);
    let receiver = match UnixDatagram::bind(&path) {
        Ok(receiver) => receiver,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("bind receiver: {error}"),
    };
    let record = ServiceLogRecord::new("app", LogStream::Stdout, "hello", 42, None);

    send_eventd_log_record(path.to_str().expect("utf8 path"), &record).expect("send record");

    let mut buffer = [0_u8; 512];
    let len = receiver.recv(&mut buffer).expect("receive datagram");
    let datagram = &buffer[..len];
    assert!(datagram.windows(b"origin".len()).any(|w| w == b"origin"));
    assert!(datagram.windows(b"app".len()).any(|w| w == b"app"));
    assert!(datagram.windows(b"message".len()).any(|w| w == b"message"));
    assert!(datagram.windows(b"hello".len()).any(|w| w == b"hello"));

    drop(receiver);
    let _ = std::fs::remove_file(&path);
}
