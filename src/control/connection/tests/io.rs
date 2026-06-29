use std::collections::VecDeque;
use std::io;

use crate::control::connection::{
    ControlConnectionIo, ControlConnectionReadTurn, ControlConnectionState,
    ControlConnectionWriteTurn, flush_control_connection, read_control_connection,
};
use crate::control::socket::{
    ControlSocketRead, ControlSocketReadError, ControlSocketWrite, ControlSocketWriteError,
};

#[test]
fn read_turn_appends_bytes_to_connection_buffer() {
    let mut io = FakeConnectionIo::reads([ControlSocketRead::Bytes(b"abc".to_vec())]);
    let mut state = ControlConnectionState::new();

    let turn = read_control_connection(&mut io, &mut state, 64).expect("read");

    assert_eq!(
        turn,
        ControlConnectionReadTurn::Bytes {
            read_bytes: 3,
            buffered_bytes: 3,
        },
    );
    assert_eq!(state.read_buffer().as_slice(), b"abc");
    assert_eq!(io.read_sizes, vec![64]);
}

#[test]
fn read_turn_reports_would_block_without_mutating_buffer() {
    let mut io = FakeConnectionIo::reads([ControlSocketRead::WouldBlock]);
    let mut state = ControlConnectionState::new();
    state.read_buffer_mut().append(b"pending");

    let turn = read_control_connection(&mut io, &mut state, 16).expect("read");

    assert_eq!(
        turn,
        ControlConnectionReadTurn::WouldBlock {
            buffered_bytes: b"pending".len(),
        },
    );
    assert_eq!(state.read_buffer().as_slice(), b"pending");
}

#[test]
fn flush_turn_advances_partial_write_and_preserves_close_flag() {
    let mut io = FakeConnectionIo::writes([ControlSocketWrite::Partial { written: 5 }]);
    let mut state = ControlConnectionState::new();
    state.enqueue_response(b"{\"status\":\"ok\"}\n", true);

    let turn = flush_control_connection(&mut io, &mut state).expect("flush");

    assert_eq!(
        turn,
        ControlConnectionWriteTurn::Partial {
            written: 5,
            pending_bytes: b"tus\":\"ok\"}\n".len(),
        },
    );
    assert_eq!(state.pending_write_bytes(), b"tus\":\"ok\"}\n".len());
    assert!(state.close_after_write());
    assert_eq!(io.writes, vec![b"{\"status\":\"ok\"}\n".to_vec()]);
}

#[test]
fn flush_turn_reports_complete_and_retains_close_after_write_decision() {
    let mut io = FakeConnectionIo::writes([ControlSocketWrite::Complete]);
    let mut state = ControlConnectionState::new();
    state.enqueue_response(b"{\"status\":\"error\"}\n", true);

    let turn = flush_control_connection(&mut io, &mut state).expect("flush");

    assert_eq!(
        turn,
        ControlConnectionWriteTurn::Complete {
            written: b"{\"status\":\"error\"}\n".len(),
            close_after_write: true,
        },
    );
    assert_eq!(state.pending_write_bytes(), 0);
    assert!(state.close_after_write());
}

#[test]
fn flush_turn_is_idle_when_no_response_is_pending() {
    let mut io = FakeConnectionIo::default();
    let mut state = ControlConnectionState::new();
    state.mark_close_after_write();

    let turn = flush_control_connection(&mut io, &mut state).expect("flush");

    assert_eq!(
        turn,
        ControlConnectionWriteTurn::Idle {
            close_after_write: true,
        },
    );
    assert!(io.writes.is_empty());
}

#[derive(Debug, Default)]
struct FakeConnectionIo {
    reads: VecDeque<ControlSocketRead>,
    write_results: VecDeque<ControlSocketWrite>,
    read_sizes: Vec<usize>,
    writes: Vec<Vec<u8>>,
}

impl FakeConnectionIo {
    fn reads(reads: impl IntoIterator<Item = ControlSocketRead>) -> Self {
        Self {
            reads: reads.into_iter().collect(),
            ..Self::default()
        }
    }

    fn writes(writes: impl IntoIterator<Item = ControlSocketWrite>) -> Self {
        Self {
            write_results: writes.into_iter().collect(),
            ..Self::default()
        }
    }
}

impl ControlConnectionIo for FakeConnectionIo {
    fn read_control(
        &mut self,
        max_bytes: usize,
    ) -> Result<ControlSocketRead, ControlSocketReadError> {
        self.read_sizes.push(max_bytes);
        Ok(self.reads.pop_front().unwrap_or(ControlSocketRead::Eof))
    }

    fn write_control(
        &mut self,
        bytes: &[u8],
    ) -> Result<ControlSocketWrite, ControlSocketWriteError> {
        self.writes.push(bytes.to_vec());
        self.write_results.pop_front().ok_or_else(|| {
            ControlSocketWriteError::Write(io::Error::new(
                io::ErrorKind::WouldBlock,
                "scripted write exhausted",
            ))
        })
    }
}
