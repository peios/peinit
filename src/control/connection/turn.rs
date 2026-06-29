use crate::control::socket::ControlSocketWriteError;
use crate::control::wire::ControlWriteBufferError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlConnectionReadTurn {
    Bytes {
        read_bytes: usize,
        buffered_bytes: usize,
    },
    Eof,
    WouldBlock {
        buffered_bytes: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlConnectionWriteTurn {
    Idle {
        close_after_write: bool,
    },
    Complete {
        written: usize,
        close_after_write: bool,
    },
    Partial {
        written: usize,
        pending_bytes: usize,
    },
    WouldBlock {
        pending_bytes: usize,
    },
}

#[derive(Debug)]
pub enum ControlConnectionWriteTurnError {
    Socket(ControlSocketWriteError),
    Buffer(ControlWriteBufferError),
}
