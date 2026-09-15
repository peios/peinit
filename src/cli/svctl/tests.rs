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

#[test]
fn job_list_sends_filters_and_renders_a_table() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "job-list");
            assert_eq!(request["state"], "running");
            assert_eq!(request["submitter"], "S-1-5-21-1-2-3-1001");
            assert!(request.get("identity").is_none());
        },
        r#"{"status":"ok","jobs":[{"id":"job-1","type":"submitted","state":"running","submitter":"S-1-5-21-1-2-3-1001","identity":"S-1-5-21-1-2-3-1001","description":"nightly","progress":{"current":3,"total":5,"bounded":true,"unit":"items"}}]}"#,
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
            OsString::from("job"),
            OsString::from("list"),
            OsString::from("--state=running"),
            OsString::from("--submitter"),
            OsString::from("S-1-5-21-1-2-3-1001"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    let out = String::from_utf8(out).expect("stdout utf8");
    assert!(out.starts_with("JOB"), "{out}");
    assert!(out.contains("job-1"));
    assert!(out.contains("3/5 items"));
    assert!(out.contains("nightly"));
    assert!(err.is_empty());
    server.join();
}

#[test]
fn job_status_renders_the_view() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "job-status");
            assert_eq!(request["job_id"], "job-1");
        },
        r#"{"status":"ok","job":{"id":"job-1","type":"submitted","state":"failed","cause":"timeout","submitter":"S-1-5-18","identity":"S-1-5-21-1-2-3-1001","logon_session":999,"description":"","image_path":"/usr/bin/backup","pid":null,"ready":null,"exit_code":null,"exit_signal":15,"status_text":"Backing up /data","progress":{"current":7,"total":null,"bounded":true,"unit":null},"created_at":"2026-08-29T10:00:00Z","started_at":"2026-08-29T10:00:01Z","ended_at":"2026-08-29T10:30:01Z"}}"#,
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
            OsString::from("job"),
            OsString::from("status"),
            OsString::from("job-1"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8(out).expect("stdout utf8"),
        "job job-1: failed\n\
         cause: timeout\n\
         image: /usr/bin/backup\n\
         submitter: S-1-5-18\n\
         identity: S-1-5-21-1-2-3-1001\n\
         logon session: 0x3e7\n\
         status: Backing up /data\n\
         progress: 7/?\n\
         exit signal: 15\n\
         created: 2026-08-29T10:00:00Z\n\
         started: 2026-08-29T10:00:01Z\n\
         ended: 2026-08-29T10:30:01Z\n",
    );
    assert!(err.is_empty());
    server.join();
}

#[test]
fn job_stop_defaults_to_waiting() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "job-stop");
            assert_eq!(request["job_id"], "job-1");
            assert_eq!(request["wait"], true);
        },
        r#"{"status":"ok","job":{"id":"job-1","state":"failed","cause":"explicit_stop"}}"#,
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
            OsString::from("job"),
            OsString::from("stop"),
            OsString::from("job-1"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    assert!(
        String::from_utf8(out)
            .expect("stdout utf8")
            .contains("explicit_stop")
    );
    assert!(err.is_empty());
    server.join();
}

#[test]
fn job_submit_builds_the_definition_from_its_options() {
    use super::args::{ParseOutcome, parse};
    use super::command::Command;
    use super::execute::submission_definition;

    let parsed = parse([
        "svctl",
        "--wait",
        "job",
        "submit",
        "--description=nightly",
        "--cwd",
        "/var/backups",
        "--env",
        "MODE=full",
        "--env=LEVEL=3",
        "--timeout",
        "3600",
        "--stop-timeout=30",
        "--readiness",
        "notify",
        "--readiness-timeout=20",
        "--success-exit-code",
        "0",
        "--success-exit-code",
        "3",
        "--fd",
        "control=7",
        "--output",
        "--security-descriptor",
        "O:BAG:BAD:(A;;0x7;;;BA)",
        "/usr/bin/backup",
        "--full",
        "--target=/mnt",
    ])
    .expect("parse");
    let ParseOutcome::Run(invocation) = parsed else {
        panic!("expected run");
    };
    let Command::JobSubmit { submission, wait } = invocation.command else {
        panic!("expected job submit, got {:?}", invocation.command);
    };
    assert!(wait);
    assert_eq!(submission.descriptors, vec![("control".to_string(), 7)]);
    assert!(submission.output);
    let definition = submission_definition(&submission);
    assert_eq!(
        definition,
        serde_json::json!({
            "image_path": "/usr/bin/backup",
            "arguments": ["--full", "--target=/mnt"],
            "environment": {"MODE": "full", "LEVEL": "3"},
            "working_directory": "/var/backups",
            "description": "nightly",
            "timeout": 3600,
            "stop_timeout": 30,
            "readiness": "notify",
            "readiness_timeout": 20,
            "success_exit_codes": [0, 3],
            "descriptors": ["control"],
            "output": true,
            "security_descriptor": "O:BAG:BAD:(A;;0x7;;;BA)",
        })
    );
}

#[test]
fn job_submit_rejects_a_relative_image_and_a_bad_fd() {
    use super::args::parse;

    let error = parse(["svctl", "job", "submit", "backup"]).expect_err("relative image");
    assert!(error.message.contains("absolute"));
    let error = parse([
        "svctl",
        "job",
        "submit",
        "--fd",
        "control",
        "/usr/bin/backup",
    ])
    .expect_err("fd");
    assert!(error.message.contains("NAME=FD"));
    let error = parse(["svctl", "job", "submit"]).expect_err("no image");
    assert!(error.message.contains("image path"));
}

#[test]
fn job_wait_and_signal_parse_their_arguments() {
    use super::args::{ParseOutcome, parse};
    use super::command::Command;

    let ParseOutcome::Run(invocation) =
        parse(["svctl", "job", "wait", "--for", "ready", "job-1"]).expect("parse")
    else {
        panic!("expected run");
    };
    assert_eq!(
        invocation.command,
        Command::JobWait {
            job_id: "job-1".to_string(),
            for_ready: true,
        }
    );
    let ParseOutcome::Run(invocation) =
        parse(["svctl", "job", "signal", "job-1", "SIGUSR1"]).expect("parse")
    else {
        panic!("expected run");
    };
    assert_eq!(
        invocation.command,
        Command::JobSignal {
            job_id: "job-1".to_string(),
            signal: libc::SIGUSR1,
        }
    );
    let error = parse(["svctl", "--wait", "job", "wait", "job-1"]).expect_err("usage");
    assert!(error.message.contains("not valid for job wait"));
    let error = parse(["svctl", "job", "signal", "job-1", "0"]).expect_err("usage");
    assert!(error.message.contains("signal number or name"));
}

#[test]
fn jobs_socket_path_is_a_separate_global() {
    use super::args::{ParseOutcome, parse};

    let ParseOutcome::Run(invocation) =
        parse(["svctl", "--jobs-socket=/tmp/j.sock", "job", "wait", "job-1"]).expect("parse")
    else {
        panic!("expected run");
    };
    assert_eq!(
        invocation.jobs_socket_path,
        std::path::PathBuf::from("/tmp/j.sock")
    );
    assert_eq!(
        invocation.socket_path,
        std::path::PathBuf::from(crate::control::socket::CONTROL_SOCKET_PATH)
    );
}

/// PEI-621: the human rendering of a reload says which keys would not
/// decode, and why, rather than leaving the operator with a count.
#[test]
fn human_reload_config_lists_undecodable_definitions() {
    let Some(server) = MockControlServer::start(
        |request| {
            assert_eq!(request["command"], "reload-config");
        },
        r#"{"status":"ok","summary":{"added":["new"],"updated":[],"restored":[],"marked_removed":[],"discarded":[],"undecodable":["broken","worse"],"deferred":["app"]},"undecodable":[{"service":"broken","field":"ImagePath","message":"MalformedString { field: \"ImagePath\", reason: MissingTerminator }"},{"service":"worse","field":null,"message":"MissingImagePath"}],"warnings":[]}"#,
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
            OsString::from("reload-config"),
        ],
        &mut out,
        &mut err,
    );

    assert_eq!(code, 0);
    let out = String::from_utf8(out).expect("stdout utf8");
    assert!(out.starts_with("configuration reloaded\n"), "{out}");
    assert!(out.contains("added: 1\n"), "{out}");
    assert!(out.contains("undecodable: 2\n"), "{out}");
    assert!(out.contains("deferred: 1\n"), "{out}");
    assert!(
        out.contains(
            "undecodable definitions:\n  broken (ImagePath): MalformedString { field: \"ImagePath\", reason: MissingTerminator }\n  worse: MissingImagePath\n"
        ),
        "{out}"
    );
    assert!(err.is_empty());
    server.join();
}
