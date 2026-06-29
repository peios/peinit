use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde_json::{Value, json};

use crate::shutdown::ShutdownKind;

use super::socket::CONTROL_SOCKET_PATH;
use super::wire::ControlResponseStatus;

const MAX_CONTROL_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct ControlClient {
    stream: UnixStream,
    read_buffer: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ControlResponse {
    raw_json: String,
    value: Value,
    status: ControlResponseStatus,
    error_code: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug)]
pub enum ControlClientError {
    Io(String),
    Protocol(String),
}

impl ControlClient {
    pub fn connect_default() -> Result<Self, ControlClientError> {
        Self::connect_path(CONTROL_SOCKET_PATH)
    }

    pub fn connect_path(path: impl AsRef<Path>) -> Result<Self, ControlClientError> {
        let path = path.as_ref();
        let stream = UnixStream::connect(path).map_err(|error| {
            ControlClientError::Io(format!("connect {}: {error}", path.display()))
        })?;
        Ok(Self {
            stream,
            read_buffer: Vec::new(),
        })
    }

    pub fn request(&mut self, request: Value) -> Result<ControlResponse, ControlClientError> {
        let mut line = serde_json::to_vec(&request)
            .map_err(|error| ControlClientError::Protocol(format!("encode request: {error}")))?;
        line.push(b'\n');
        self.stream
            .write_all(&line)
            .map_err(|error| ControlClientError::Io(format!("write control request: {error}")))?;

        let response_line = self.read_response_line()?;
        ControlResponse::from_line(&response_line)
    }

    pub fn service_command(
        &mut self,
        command: &'static str,
        service: &str,
        wait: bool,
    ) -> Result<ControlResponse, ControlClientError> {
        self.request(json!({
            "command": command,
            "service": service,
            "wait": wait,
        }))
    }

    pub fn service_start(
        &mut self,
        service: &str,
        wait: bool,
    ) -> Result<ControlResponse, ControlClientError> {
        self.service_command("start", service, wait)
    }

    pub fn service_stop(
        &mut self,
        service: &str,
        wait: bool,
    ) -> Result<ControlResponse, ControlClientError> {
        self.service_command("stop", service, wait)
    }

    pub fn service_restart(
        &mut self,
        service: &str,
        wait: bool,
    ) -> Result<ControlResponse, ControlClientError> {
        self.service_command("restart", service, wait)
    }

    pub fn service_reload(
        &mut self,
        service: &str,
        wait: bool,
    ) -> Result<ControlResponse, ControlClientError> {
        self.service_command("reload", service, wait)
    }

    pub fn service_reset(&mut self, service: &str) -> Result<ControlResponse, ControlClientError> {
        self.service_command("reset", service, false)
    }

    pub fn service_status(&mut self, service: &str) -> Result<ControlResponse, ControlClientError> {
        self.service_command("status", service, false)
    }

    pub fn service_list(&mut self) -> Result<ControlResponse, ControlClientError> {
        self.request(json!({ "command": "list" }))
    }

    pub fn operation_status(
        &mut self,
        operation_id: &str,
    ) -> Result<ControlResponse, ControlClientError> {
        self.request(json!({
            "command": "operation-status",
            "operation_id": operation_id,
        }))
    }

    pub fn reload_config(&mut self) -> Result<ControlResponse, ControlClientError> {
        self.request(json!({ "command": "reload-config" }))
    }

    pub fn system_shutdown(
        &mut self,
        shutdown_kind: ShutdownKind,
    ) -> Result<ControlResponse, ControlClientError> {
        self.request(json!({
            "command": "shutdown",
            "type": shutdown_kind_wire(shutdown_kind),
        }))
    }

    fn read_response_line(&mut self) -> Result<Vec<u8>, ControlClientError> {
        loop {
            if let Some(newline) = self.read_buffer.iter().position(|byte| *byte == b'\n') {
                if newline == 0 {
                    return Err(ControlClientError::Protocol(
                        "empty control response".to_string(),
                    ));
                }
                let line = self.read_buffer[..newline].to_vec();
                self.read_buffer.drain(..=newline);
                return Ok(line);
            }
            if self.read_buffer.len() > MAX_CONTROL_RESPONSE_BYTES {
                return Err(ControlClientError::Protocol(
                    "control response exceeded maximum client buffer".to_string(),
                ));
            }

            let mut chunk = [0_u8; 4096];
            let read = self.stream.read(&mut chunk).map_err(|error| {
                ControlClientError::Io(format!("read control response: {error}"))
            })?;
            if read == 0 {
                return Err(ControlClientError::Protocol(
                    "control socket closed before a complete response".to_string(),
                ));
            }
            self.read_buffer.extend_from_slice(&chunk[..read]);
        }
    }
}

impl ControlResponse {
    pub fn raw_json(&self) -> &str {
        &self.raw_json
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn status(&self) -> ControlResponseStatus {
        self.status
    }

    pub fn is_ok(&self) -> bool {
        self.status == ControlResponseStatus::Ok
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    fn from_line(line: &[u8]) -> Result<Self, ControlClientError> {
        let value: Value = serde_json::from_slice(line).map_err(|_| {
            ControlClientError::Protocol("control response is not valid JSON".to_string())
        })?;
        let object = value.as_object().ok_or_else(|| {
            ControlClientError::Protocol("control response is not a JSON object".to_string())
        })?;
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ControlClientError::Protocol("control response is missing status".to_string())
            })?;
        let status = ControlResponseStatus::parse(status).ok_or_else(|| {
            ControlClientError::Protocol("control response has unknown status".to_string())
        })?;
        let raw_json = std::str::from_utf8(line)
            .map_err(|_| {
                ControlClientError::Protocol("control response is not valid UTF-8".to_string())
            })?
            .to_string();
        let error_code = object
            .get("code")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let error_message = object
            .get("message")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        Ok(Self {
            raw_json,
            value,
            status,
            error_code,
            error_message,
        })
    }
}

impl fmt::Display for ControlClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Protocol(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ControlClientError {}

fn shutdown_kind_wire(kind: ShutdownKind) -> &'static str {
    match kind {
        ShutdownKind::Poweroff => "poweroff",
        ShutdownKind::Reboot => "reboot",
        ShutdownKind::Halt => "halt",
    }
}

#[cfg(test)]
mod tests {
    use std::io::{ErrorKind, Read, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::Value;

    use super::*;

    #[test]
    fn sends_request_and_parses_ok_response() {
        let path = temp_socket_path("peinit-control-client");
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

        let mut client = ControlClient::connect_path(&path).expect("connect");
        let response = client.service_start("api", true).expect("response");
        assert!(response.is_ok());
        assert_eq!(
            response.raw_json(),
            r#"{"status":"ok","service":"api","state":"active"}"#,
        );
        assert_eq!(response.value()["state"], "active");

        server.join().expect("mock control server");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn exposes_peinit_error_envelope() {
        let path = temp_socket_path("peinit-control-client-error");
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

        let mut client = ControlClient::connect_path(&path).expect("connect");
        let response = client.service_status("missing").expect("response");
        assert!(!response.is_ok());
        assert_eq!(response.status(), ControlResponseStatus::Error);
        assert_eq!(response.error_code(), Some("UNKNOWN_SERVICE"));
        assert_eq!(response.error_message(), Some("missing"));

        server.join().expect("mock control server");
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

    fn temp_socket_path(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.sock", std::process::id()))
    }
}
