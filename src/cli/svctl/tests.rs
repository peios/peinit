use std::ffi::OsString;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::execute::run_with_io;

#[test]
fn json_start_sends_no_wait_request() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "start");
            assert_eq!(request["service"], "api");
            assert_eq!(request["wait"], false);
        },
        r#"{"status":"ok","operation_id":"op-1","service":"api","state":"starting","warnings":[]}"#,
    ) else {
        return;
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        [
            OsString::from("svctl"),
            OsString::from("--socket"),
            server.path().into_os_string(),
            OsString::from("--json"),
            OsString::from("--no-wait"),
            OsString::from("start"),
            OsString::from("api"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8(out).expect("stdout utf8"),
        "{\"status\":\"ok\",\"operation_id\":\"op-1\",\"service\":\"api\",\"state\":\"starting\",\"warnings\":[]}\n",
    );
    assert!(err.is_empty());
    server.join();
}

#[test]
fn human_list_renders_service_table() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "list");
        },
        r#"{"status":"ok","services":[{"service":"api","state":"active","health":"healthy","cause":"explicit_start"},{"service":"db","state":"inactive","health":null,"cause":null}]}"#,
    ) else {
        return;
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        [
            OsString::from("svctl"),
            OsString::from("--socket"),
            server.path().into_os_string(),
            OsString::from("list"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    let out = String::from_utf8(out).expect("stdout utf8");
    assert!(out.contains("SERVICE"));
    assert!(out.contains("api"));
    assert!(out.contains("active"));
    assert!(out.contains("db"));
    assert!(err.is_empty());
    server.join();
}

#[test]
fn server_error_returns_failure_exit() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "status");
            assert_eq!(request["service"], "missing");
        },
        r#"{"status":"error","code":"UNKNOWN_SERVICE","message":"missing"}"#,
    ) else {
        return;
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        [
            OsString::from("svctl"),
            OsString::from("--socket"),
            server.path().into_os_string(),
            OsString::from("status"),
            OsString::from("missing"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 1);
    assert!(out.is_empty());
    let err = String::from_utf8(err).expect("stderr utf8");
    assert!(err.contains("UNKNOWN_SERVICE"));
    assert!(err.contains("missing"));
    server.join();
}

#[test]
fn reboot_multicall_sends_shutdown_request() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "shutdown");
            assert_eq!(request["type"], "reboot");
        },
        r#"{"status":"ok"}"#,
    ) else {
        return;
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        [
            OsString::from("reboot"),
            OsString::from("--socket"),
            server.path().into_os_string(),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8(out).expect("stdout utf8"),
        "shutdown requested: reboot\n",
    );
    assert!(err.is_empty());
    server.join();
}

#[test]
fn classic_shutdown_reboot_form_sends_shutdown_request() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "shutdown");
            assert_eq!(request["type"], "reboot");
        },
        r#"{"status":"ok"}"#,
    ) else {
        return;
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        [
            OsString::from("shutdown"),
            OsString::from("--socket"),
            server.path().into_os_string(),
            OsString::from("-r"),
            OsString::from("now"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8(out).expect("stdout utf8"),
        "shutdown requested: reboot\n",
    );
    assert!(err.is_empty());
    server.join();
}

struct MockControlServer {
    path: std::path::PathBuf,
    handle: thread::JoinHandle<()>,
}

impl MockControlServer {
    fn start<F>(assert_request: F, response: &'static str) -> Option<Self>
    where
        F: FnOnce(Value) + Send + 'static,
    {
        let path = temp_socket_path("svctl-control");
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("bind mock control socket: {error}"),
        };
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept control client");
            let request = read_line(&mut stream);
            let request: Value = serde_json::from_slice(&request).expect("request json");
            assert_request(request);
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            stream.write_all(b"\n").expect("write response newline");
        });
        Some(Self { path, handle })
    }

    fn path(&self) -> std::path::PathBuf {
        self.path.clone()
    }

    fn join(self) {
        self.handle.join().expect("mock control server");
        let _ = std::fs::remove_file(self.path);
    }
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

fn temp_socket_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.sock", std::process::id()))
}
