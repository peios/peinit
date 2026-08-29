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

#[test]
fn job_status_over_the_control_socket_returns_the_view() {
    let path = temp_socket_path("libpeinit-job-status");
    let Some(listener) = bind_listener_or_skip(&path) else {
        return;
    };
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept control client");
        let request = read_line(&mut stream);
        let value: Value = serde_json::from_slice(&request).expect("request json");
        assert_eq!(value["command"], "job-status");
        assert_eq!(value["job_id"], "job-1");
        stream
            .write_all(br#"{"status":"ok","job":{"id":"job-1","state":"running"}}"#)
            .expect("write response");
        stream.write_all(b"\n").expect("write response newline");
    });

    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let job_c = CString::new("job-1").expect("job cstring");
    let mut client: *mut peinit_client_t = std::ptr::null_mut();
    let mut response: *mut peinit_response_t = std::ptr::null_mut();
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_client_connect_path(path_c.as_ptr(), &mut client, &mut error),
            PEINIT_OK,
        );
        assert_eq!(
            peinit_job_status(client, job_c.as_ptr(), &mut response, &mut error),
            PEINIT_OK,
        );
        assert!(error.is_null());
        assert_eq!(peinit_response_is_ok(response), 1);
        assert_eq!(
            borrowed_str(peinit_response_json(response)),
            r#"{"status":"ok","job":{"id":"job-1","state":"running"}}"#,
        );
        assert_eq!(
            peinit_response_take_pidfd(response),
            -1,
            "a control response never carries a handle"
        );
        peinit_response_free(response);
        peinit_client_free(client);
    }

    server.join().expect("mock control server");
    let _ = std::fs::remove_file(path);
}

#[test]
fn job_list_rejects_a_filter_that_is_not_a_job_list_filter() {
    let path = temp_socket_path("libpeinit-job-list");
    let Some(listener) = bind_listener_or_skip(&path) else {
        return;
    };
    // The refusal is local: the server never sees a request.
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept control client");
        drop(stream);
    });

    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let filter_c = CString::new(r#"{"service":"app"}"#).expect("filter cstring");
    let mut client: *mut peinit_client_t = std::ptr::null_mut();
    let mut response: *mut peinit_response_t = std::ptr::null_mut();
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_client_connect_path(path_c.as_ptr(), &mut client, &mut error),
            PEINIT_OK,
        );
        assert_eq!(
            peinit_job_list(client, filter_c.as_ptr(), &mut response, &mut error),
            PEINIT_ERR_INVALID_ARGUMENT,
        );
        assert!(response.is_null());
        assert!(!error.is_null());
        assert!(borrowed_str(peinit_error_message(error)).contains("service"));
        peinit_error_free(error);
        peinit_client_free(client);
    }

    server.join().expect("mock control server");
    let _ = std::fs::remove_file(path);
}

#[test]
fn jobs_client_reports_a_missing_socket_as_io() {
    let path = temp_socket_path("libpeinit-jobs-missing");
    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let mut jobs: *mut peinit_jobs_t = std::ptr::null_mut();
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_jobs_connect_path(path_c.as_ptr(), &mut jobs, &mut error),
            PEINIT_ERR_IO,
        );
        assert!(jobs.is_null());
        assert!(!error.is_null());
        assert_eq!(peinit_jobs_fd(jobs), -1);
        peinit_error_free(error);
    }
}

#[test]
fn job_submit_validates_its_arguments_before_sending() {
    let path = temp_socket_path("libpeinit-jobs-args");
    let Some(listener) = seqpacket_listener_or_skip(&path) else {
        return;
    };
    let path_c = CString::new(path.to_str().expect("utf8 temp path")).expect("path cstring");
    let mut jobs: *mut peinit_jobs_t = std::ptr::null_mut();
    let mut response: *mut peinit_response_t = std::ptr::null_mut();
    let mut error: *mut peinit_error_t = std::ptr::null_mut();

    unsafe {
        assert_eq!(
            peinit_jobs_connect_path(path_c.as_ptr(), &mut jobs, &mut error),
            PEINIT_OK,
        );
        assert!(peinit_jobs_fd(jobs) >= 0);

        let not_object = CString::new("[]").expect("cstring");
        assert_eq!(
            peinit_job_submit(
                jobs,
                not_object.as_ptr(),
                -1,
                std::ptr::null(),
                0,
                &mut response,
                &mut error
            ),
            PEINIT_ERR_INVALID_ARGUMENT,
        );
        peinit_error_free(error);

        let definition = CString::new(r#"{"image_path":"/bin/true"}"#).expect("cstring");
        let bad_fds = [-1_i32];
        assert_eq!(
            peinit_job_submit(
                jobs,
                definition.as_ptr(),
                -1,
                bad_fds.as_ptr(),
                bad_fds.len(),
                &mut response,
                &mut error
            ),
            PEINIT_ERR_INVALID_ARGUMENT,
        );
        peinit_error_free(error);
        assert_eq!(
            peinit_job_submit(
                jobs,
                definition.as_ptr(),
                -2,
                std::ptr::null(),
                0,
                &mut response,
                &mut error
            ),
            PEINIT_ERR_INVALID_ARGUMENT,
        );
        peinit_error_free(error);
        assert!(response.is_null());
        peinit_jobs_free(jobs);
    }
    drop(listener);
    let _ = std::fs::remove_file(path);
}

/// A bound SOCK_SEQPACKET listener, or None where the sandbox forbids one.
fn seqpacket_listener_or_skip(path: &std::path::Path) -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;

    let address = crate::control::socket::address::unix_socket_address(path).ok()?;
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return None;
    }
    let socket = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
    let bound = unsafe {
        libc::bind(
            fd,
            (&address.addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            address.len,
        )
    };
    if bound < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == ErrorKind::PermissionDenied {
            return None;
        }
        panic!("bind seqpacket listener: {error}");
    }
    if unsafe { libc::listen(fd, 1) } < 0 {
        panic!("listen: {}", std::io::Error::last_os_error());
    }
    Some(socket)
}
