use std::ffi::{CStr, CString};
use std::io::ErrorKind;
use std::io::{Read, Write};
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::*;

#[test]
fn control_client_sends_command_and_exposes_response() {
    let path = temp_socket_path("libpeinit-control");
    let Some(listener) = bind_listener_or_skip(&path) else {
        return;
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept control client");
        let request = read_line(&mut stream);
        let value: Value = serde_json::from_slice(&request).expect("request json");
        assert_eq!(value["command"], "start");
        assert_eq!(value["service"], "api");
        assert_eq!(value["wait"], true);
        stream
            .write_all(br#"{"status":"ok","service":"api","state":"active"}"#)
            .expect("write response");
        stream.write_all(b"\n").expect("write response newline");
    });

    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let service_c = CString::new("api").expect("service cstring");
    let mut client: *mut peinit_client_t = std::ptr::null_mut();
    let mut response: *mut peinit_response_t = std::ptr::null_mut();
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_client_connect_path(path_c.as_ptr(), &mut client, &mut error),
            PEINIT_OK,
        );
        assert!(error.is_null());
        assert!(!client.is_null());

        assert_eq!(
            peinit_service_start(client, service_c.as_ptr(), true, &mut response, &mut error),
            PEINIT_OK,
        );
        assert!(error.is_null());
        assert!(!response.is_null());
        assert_eq!(peinit_response_is_ok(response), 1);
        assert_eq!(borrowed_str(peinit_response_status(response)), "ok");
        assert_eq!(
            borrowed_str(peinit_response_json(response)),
            r#"{"status":"ok","service":"api","state":"active"}"#,
        );

        peinit_response_free(response);
        peinit_client_free(client);
    }

    server.join().expect("mock control server");
    let _ = std::fs::remove_file(path);
}

#[test]
fn response_object_exposes_peinit_error_envelope() {
    let path = temp_socket_path("libpeinit-control-error");
    let Some(listener) = bind_listener_or_skip(&path) else {
        return;
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept control client");
        let _ = read_line(&mut stream);
        stream
            .write_all(br#"{"status":"error","code":"UNKNOWN_SERVICE","message":"missing"}"#)
            .expect("write response");
        stream.write_all(b"\n").expect("write response newline");
    });

    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let request_c =
        CString::new(r#"{"command":"status","service":"missing"}"#).expect("request cstring");
    let mut client: *mut peinit_client_t = std::ptr::null_mut();
    let mut response: *mut peinit_response_t = std::ptr::null_mut();
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_client_connect_path(path_c.as_ptr(), &mut client, &mut error),
            PEINIT_OK,
        );
        assert_eq!(
            peinit_control_raw_json(client, request_c.as_ptr(), &mut response, &mut error),
            PEINIT_OK,
        );
        assert_eq!(peinit_response_is_ok(response), 0);
        assert_eq!(
            borrowed_str(peinit_response_error_code(response)),
            "UNKNOWN_SERVICE",
        );
        assert_eq!(
            borrowed_str(peinit_response_error_message(response)),
            "missing"
        );

        peinit_response_free(response);
        peinit_client_free(client);
    }

    server.join().expect("mock control server");
    let _ = std::fs::remove_file(path);
}

#[test]
fn notify_send_to_writes_datagram_to_explicit_socket() {
    let path = temp_socket_path("libpeinit-notify");
    let Some(socket) = bind_datagram_or_skip(&path) else {
        return;
    };
    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let message_c = CString::new("READY=1\nSTATUS=online").expect("message cstring");
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_notify_send_to(path_c.as_ptr(), message_c.as_ptr(), &mut error),
            PEINIT_OK,
        );
        assert!(error.is_null());
    }

    let mut buffer = [0_u8; 128];
    let len = socket.recv(&mut buffer).expect("receive notify datagram");
    assert_eq!(&buffer[..len], b"READY=1\nSTATUS=online");
    let _ = std::fs::remove_file(path);
}

#[test]
fn notify_rejects_malformed_payload_before_sending() {
    let path = temp_socket_path("libpeinit-notify-bad");
    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let message_c = CString::new("not-a-field").expect("message cstring");
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_notify_send_to(path_c.as_ptr(), message_c.as_ptr(), &mut error),
            PEINIT_ERR_INVALID_ARGUMENT,
        );
        assert!(!error.is_null());
        assert_eq!(peinit_error_code(error), PEINIT_ERR_INVALID_ARGUMENT);
        assert!(
            borrowed_str(peinit_error_message(error)).contains("invalid notify message"),
            "unexpected error message",
        );
        peinit_error_free(error);
    }

    let _ = std::fs::remove_file(path);
}

fn read_line(stream: &mut std::os::unix::net::UnixStream) -> Vec<u8> {
    let mut line = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).expect("read request byte");
        if byte[0] == b'\n' {
            return line;
        }
        line.push(byte[0]);
    }
}

fn bind_listener_or_skip(path: &std::path::Path) -> Option<UnixListener> {
    match UnixListener::bind(path) {
        Ok(listener) => Some(listener),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => None,
        Err(error) => panic!("bind mock control socket: {error}"),
    }
}

fn bind_datagram_or_skip(path: &std::path::Path) -> Option<UnixDatagram> {
    match UnixDatagram::bind(path) {
        Ok(socket) => Some(socket),
        Err(error) if error.kind() == ErrorKind::PermissionDenied => None,
        Err(error) => panic!("bind mock notify socket: {error}"),
    }
}

unsafe fn borrowed_str<'a>(value: *const libc::c_char) -> &'a str {
    assert!(!value.is_null());
    unsafe { CStr::from_ptr(value) }
        .to_str()
        .expect("borrowed C string")
}

fn temp_socket_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.sock", std::process::id()))
}
