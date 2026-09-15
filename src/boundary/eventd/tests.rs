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

// PEI-357. The classification the lossy design turns on: a full receiver is a
// drop, a broken one is a transport failure. Getting this wrong in the
// permissive direction would hide a genuinely dead eventd; getting it wrong in
// the strict direction — which is what happened — flips peinit out of
// real-time forwarding every time eventd falls behind.
#[test]
fn a_full_receive_buffer_is_a_drop_and_anything_else_is_a_failure() {
    use std::io::Error;

    for errno in [libc::EAGAIN, libc::EWOULDBLOCK, libc::ENOBUFS] {
        assert!(
            super::is_receiver_full(&Error::from_raw_os_error(errno)),
            "errno {errno} should read as a full receiver",
        );
    }
    for errno in [
        libc::ECONNREFUSED,
        libc::ENOENT,
        libc::EPIPE,
        libc::EMSGSIZE,
        libc::EACCES,
    ] {
        assert!(
            !super::is_receiver_full(&Error::from_raw_os_error(errno)),
            "errno {errno} is not a full receiver",
        );
    }
}

// PEI-807. EMSGSIZE is neither a full receiver nor a broken connection: the
// datagram is too big for the socket, and sending the same one again can
// only fail the same way. Classed as a transport failure, the batch was kept
// and replayed identically every turn for the rest of the boot.
#[test]
fn a_datagram_too_large_for_the_socket_is_oversized_and_nothing_else_is() {
    use std::io::Error;

    assert!(super::is_oversized(&Error::from_raw_os_error(
        libc::EMSGSIZE
    )));
    for errno in [
        libc::EAGAIN,
        libc::ENOBUFS,
        libc::ECONNREFUSED,
        libc::ENOENT,
        libc::EPIPE,
        libc::EACCES,
    ] {
        assert!(
            !super::is_oversized(&Error::from_raw_os_error(errno)),
            "errno {errno} is not an oversized datagram",
        );
    }
}

/// `SO_SNDBUF` as the kernel reports it on the sink's connected socket.
fn reported_send_buffer(sink: &LinuxEventdLogSink) -> usize {
    let fd = sink
        .connected
        .as_ref()
        .expect("connected sender")
        .socket
        .as_raw_fd();
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&mut value as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    assert_eq!(rc, 0, "getsockopt SO_SNDBUF");
    usize::try_from(value).expect("non-negative SO_SNDBUF")
}

// PEI-807. The batches are built to the 262144-byte portable ceiling, and a
// Unix datagram socket's default send buffer does not carry that: a full
// batch failed with EMSGSIZE. The sink asks for the ceiling when it connects
// and reports back what the socket will actually carry, so the caller can
// bound its batches by it.
#[test]
fn the_sink_raises_its_send_buffer_and_reports_the_ceiling_it_got() {
    let path = std::env::temp_dir().join(format!(
        "peinit2-eventd-log-sndbuf-{}-{}.sock",
        std::process::id(),
        SOCKET_ID.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::remove_file(&path);
    let receiver = match UnixDatagram::bind(&path) {
        Ok(receiver) => receiver,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(error) => panic!("bind receiver: {error}"),
    };
    let path = path.to_str().expect("utf8 path");
    let mut sink = LinuxEventdLogSink::new();

    let ceiling = sink
        .eventd_datagram_ceiling(path)
        .expect("connect")
        .expect("a connected sink knows its ceiling");

    let reported = reported_send_buffer(&sink);
    assert_eq!(
        ceiling,
        reported - super::UNIX_DATAGRAM_SNDBUF_RESERVE,
        "the ceiling is what the kernel granted, less its own reserve",
    );
    // The kernel doubles a granted request and caps it at wmem_max; either
    // way the buffer must have moved past the stock default, which is what
    // could not carry a full batch.
    assert!(
        reported > 212_992 || reported >= 2 * crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES,
        "SO_SNDBUF {reported} was not raised",
    );

    // A batch built to the portable ceiling now goes through in one datagram
    // wherever wmem_max allows the request; where it does not, the ceiling
    // reported above is what keeps the caller from ever building one.
    if ceiling >= crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES {
        let message = "x".repeat(4000);
        let records = (0..60)
            .map(|_| ServiceLogRecord::new("app", LogStream::Stdout, &message, 42, None))
            .collect::<Vec<_>>();
        let batch_len = crate::logging::eventd_log_batch_prefix_len(
            records.iter(),
            crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES,
        );
        assert_eq!(
            batch_len,
            records.len(),
            "the batch fits the portable ceiling"
        );
        let encoded: usize = records
            .iter()
            .map(crate::logging::encoded_eventd_log_record_len)
            .sum();
        assert!(
            encoded > 212_992,
            "the batch exceeds a stock default send buffer"
        );

        let outcome = sink
            .send_eventd_log_records(path, &records)
            .expect("send the full batch");
        assert_eq!(outcome, super::EventdSendOutcome::Sent);
        let mut buffer = vec![0_u8; 2 * crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES];
        let len = receiver.recv(&mut buffer).expect("receive datagram");
        assert!(len > 212_992, "the whole batch arrived as one datagram");
    }

    drop(receiver);
    let _ = std::fs::remove_file(path);
}
