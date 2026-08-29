//! A blocking client for the jobs channel, for `svctl` and for tests.

use std::fmt;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::Path;

use serde_json::{Value, json};

use super::socket::{JOBS_SOCKET_PATH, MAX_JOBS_MESSAGE_DESCRIPTORS};
use super::wire::JobsErrorCode;

const MAX_JOBS_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct JobsClient {
    socket: OwnedFd,
}

#[derive(Debug)]
pub struct JobsResponse {
    pub raw_json: String,
    pub value: Value,
    pub ok: bool,
    pub error_code: Option<JobsErrorCode>,
    pub error_message: Option<String>,
    /// The job's process handle, when the response to a `submit` carried one.
    pub pidfd: Option<OwnedFd>,
}

impl JobsResponse {
    pub fn job(&self) -> Option<&Value> {
        self.value.get("job")
    }
}

#[derive(Debug)]
pub enum JobsClientError {
    Io(String),
    Protocol(String),
}

impl fmt::Display for JobsClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Protocol(message) => f.write_str(message),
        }
    }
}

impl JobsClient {
    pub fn connect_default() -> Result<Self, JobsClientError> {
        Self::connect_path(JOBS_SOCKET_PATH)
    }

    pub fn connect_path(path: impl AsRef<Path>) -> Result<Self, JobsClientError> {
        let path = path.as_ref();
        let stream = connect_seqpacket(path)
            .map_err(|error| JobsClientError::Io(format!("connect {}: {error}", path.display())))?;
        Ok(Self { socket: stream })
    }

    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }

    /// Send one request, attaching `token` as the job identity and `fds` as
    /// the job's descriptors, and read one response.
    pub fn request(
        &mut self,
        request: &Value,
        token: Option<BorrowedFd<'_>>,
        fds: &[BorrowedFd<'_>],
    ) -> Result<JobsResponse, JobsClientError> {
        let bytes = serde_json::to_vec(request)
            .map_err(|error| JobsClientError::Protocol(format!("encode request: {error}")))?;
        #[cfg(feature = "peios-boundary")]
        {
            peios::socket::send_message(self.socket.as_fd(), &bytes, token, fds, 0)
                .map_err(|error| JobsClientError::Io(format!("send jobs request: {error}")))?;
            let mut buffer = vec![0_u8; MAX_JOBS_RESPONSE_BYTES];
            let received =
                peios::socket::recv_message(self.socket.as_fd(), &mut buffer, 1, 0)
                    .map_err(|error| JobsClientError::Io(format!("receive jobs response: {error}")))?;
            if received.len == 0 {
                return Err(JobsClientError::Io("jobs socket closed".to_string()));
            }
            if received.truncated {
                return Err(JobsClientError::Protocol(
                    "jobs response exceeded the client buffer".to_string(),
                ));
            }
            buffer.truncate(received.len);
            let mut fds = received.fds;
            let pidfd = fds.pop();
            decode_response(&buffer, pidfd)
        }
        #[cfg(not(feature = "peios-boundary"))]
        {
            let _ = (token, fds, MAX_JOBS_MESSAGE_DESCRIPTORS);
            Err(JobsClientError::Io(
                "jobs client requires the peios boundary".to_string(),
            ))
        }
    }

    pub fn submit(
        &mut self,
        definition: Value,
        token: Option<BorrowedFd<'_>>,
        fds: &[BorrowedFd<'_>],
    ) -> Result<JobsResponse, JobsClientError> {
        if fds.len() > MAX_JOBS_MESSAGE_DESCRIPTORS {
            return Err(JobsClientError::Protocol(format!(
                "{} descriptors exceeds the {MAX_JOBS_MESSAGE_DESCRIPTORS} the manager accepts",
                fds.len()
            )));
        }
        let mut request = definition;
        request["command"] = json!("submit");
        self.request(&request, token, fds)
    }

    pub fn status(&mut self, job_id: &str) -> Result<JobsResponse, JobsClientError> {
        self.request(&json!({"command": "status", "job_id": job_id}), None, &[])
    }

    pub fn wait(&mut self, job_id: &str, for_ready: bool) -> Result<JobsResponse, JobsClientError> {
        let condition = if for_ready { "ready" } else { "terminal" };
        self.request(
            &json!({"command": "wait", "job_id": job_id, "for": condition}),
            None,
            &[],
        )
    }

    pub fn stop(&mut self, job_id: &str, wait: bool) -> Result<JobsResponse, JobsClientError> {
        self.request(
            &json!({"command": "stop", "job_id": job_id, "wait": wait}),
            None,
            &[],
        )
    }

    pub fn signal(&mut self, job_id: &str, signal: i32) -> Result<JobsResponse, JobsClientError> {
        self.request(
            &json!({"command": "signal", "job_id": job_id, "signal": signal}),
            None,
            &[],
        )
    }
}

fn connect_seqpacket(path: &Path) -> std::io::Result<OwnedFd> {
    use std::os::fd::FromRawFd;

    // std has no SOCK_SEQPACKET UnixStream; build the socket by hand.
    let address = crate::control::socket::address::unix_socket_address(path)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let socket = unsafe { OwnedFd::from_raw_fd(fd) };
    let rc = unsafe {
        libc::connect(
            fd,
            (&address.addr as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            address.len,
        )
    };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(socket)
}

fn decode_response(bytes: &[u8], pidfd: Option<OwnedFd>) -> Result<JobsResponse, JobsClientError> {
    let raw_json = String::from_utf8(bytes.to_vec())
        .map_err(|error| JobsClientError::Protocol(format!("response is not UTF-8: {error}")))?;
    let value: Value = serde_json::from_str(&raw_json)
        .map_err(|error| JobsClientError::Protocol(format!("response is not JSON: {error}")))?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| JobsClientError::Protocol("response has no status".to_string()))?;
    let ok = match status {
        "ok" => true,
        "error" => false,
        other => {
            return Err(JobsClientError::Protocol(format!(
                "response status {other:?} is not ok or error"
            )));
        }
    };
    let error_code = value
        .get("code")
        .and_then(Value::as_str)
        .map(|code| {
            JobsErrorCode::parse(code)
                .ok_or_else(|| JobsClientError::Protocol(format!("unknown error code {code:?}")))
        })
        .transpose()?;
    let error_message = value
        .get("message")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    Ok(JobsResponse {
        raw_json,
        value,
        ok,
        error_code,
        error_message,
        pidfd,
    })
}
