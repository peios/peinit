use std::io;
use std::path::PathBuf;

pub const CONTROL_SOCKET_PATH: &str = "/run/peinit/control.sock";
pub const CONTROL_SOCKET_LISTEN_BACKLOG: i32 = 32;
pub const DEFAULT_MAX_CONTROL_CONNECTIONS: usize = 32;
pub const DEFAULT_MAX_REQUEST_SIZE_BYTES: usize = 65_536;
pub const DEFAULT_CONNECTION_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSocketLimits {
    pub max_connections: usize,
    pub max_request_bytes: usize,
    pub connection_timeout_secs: u64,
}

impl Default for ControlSocketLimits {
    fn default() -> Self {
        Self {
            max_connections: DEFAULT_MAX_CONTROL_CONNECTIONS,
            max_request_bytes: DEFAULT_MAX_REQUEST_SIZE_BYTES,
            connection_timeout_secs: DEFAULT_CONNECTION_TIMEOUT_SECS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlSocketPathError {
    Empty,
    InteriorNul { index: usize },
    TooLong { len: usize, max: usize },
}

#[derive(Debug)]
pub enum ControlSocketBindError {
    Path(ControlSocketPathError),
    StalePathCleanup { path: PathBuf, source: io::Error },
    Socket(io::Error),
    Bind { path: PathBuf, source: io::Error },
    Listen(io::Error),
}

#[derive(Debug)]
pub enum ControlSocketAcceptError {
    Accept(io::Error),
}

#[derive(Debug)]
pub enum ControlSocketReadError {
    Read(io::Error),
}

#[derive(Debug)]
pub enum ControlSocketWriteError {
    Write(io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlSocketAccept {
    Accepted,
    WouldBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlSocketRead {
    Bytes(Vec<u8>),
    Eof,
    WouldBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlSocketWrite {
    Complete,
    Partial { written: usize },
    WouldBlock,
}
