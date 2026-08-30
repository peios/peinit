use std::os::fd::AsRawFd;
use std::os::unix::net::UnixDatagram;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::logging::{LogStream, ServiceLogRecord};

use super::{EventdLogSink, LinuxEventdLogSink, send_eventd_log_record};

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

#[test]
fn eventd_log_sink_reuses_its_connected_socket_and_payload_allocation() {
    let path = std::env::temp_dir().join(format!(
        "peinit2-eventd-log-reuse-{}-{}.sock",
        std::process::id(),
        SOCKET_ID.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::remove_file(&path);
    let receiver = match UnixDatagram::bind(&path) {
        Ok(receiver) => receiver,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("bind receiver: {error}"),
    };
    let records = [ServiceLogRecord::new(
        "app",
        LogStream::Stdout,
        "hello",
        42,
        None,
    )];
    let mut sink = LinuxEventdLogSink::new();
    let path = path.to_str().expect("utf8 path");

    sink.send_eventd_log_records(path, &records)
        .expect("first send");
    let first_fd = sink
        .connected
        .as_ref()
        .expect("connected sender")
        .socket
        .as_raw_fd();
    let first_capacity = sink.payload.capacity();
    sink.send_eventd_log_records(path, &records)
        .expect("second send");

    assert_eq!(
        sink.connected
            .as_ref()
            .expect("connected sender")
            .socket
            .as_raw_fd(),
        first_fd
    );
    assert_eq!(sink.payload.capacity(), first_capacity);

    let mut buffer = [0_u8; 512];
    receiver.recv(&mut buffer).expect("first datagram");
    receiver.recv(&mut buffer).expect("second datagram");
    drop(receiver);
    let _ = std::fs::remove_file(path);
}
