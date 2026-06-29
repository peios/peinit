use crate::control::socket::{
    ControlSocketRead, ControlSocketReadError, ControlSocketWrite, ControlSocketWriteError,
    LinuxControlConnection,
};

use super::{
    ControlConnectionReadTurn, ControlConnectionState, ControlConnectionWriteTurn,
    ControlConnectionWriteTurnError,
};

pub trait ControlConnectionIo {
    fn read_control(
        &mut self,
        max_bytes: usize,
    ) -> Result<ControlSocketRead, ControlSocketReadError>;

    fn write_control(
        &mut self,
        bytes: &[u8],
    ) -> Result<ControlSocketWrite, ControlSocketWriteError>;
}

impl ControlConnectionIo for LinuxControlConnection {
    fn read_control(
        &mut self,
        max_bytes: usize,
    ) -> Result<ControlSocketRead, ControlSocketReadError> {
        self.read(max_bytes)
    }

    fn write_control(
        &mut self,
        bytes: &[u8],
    ) -> Result<ControlSocketWrite, ControlSocketWriteError> {
        self.write(bytes)
    }
}

pub fn read_control_connection<I>(
    io: &mut I,
    state: &mut ControlConnectionState,
    max_read_bytes: usize,
) -> Result<ControlConnectionReadTurn, ControlSocketReadError>
where
    I: ControlConnectionIo + ?Sized,
{
    match io.read_control(max_read_bytes)? {
        ControlSocketRead::Bytes(bytes) => {
            let read_bytes = bytes.len();
            state.read_buffer_mut().append(&bytes);
            Ok(ControlConnectionReadTurn::Bytes {
                read_bytes,
                buffered_bytes: state.read_buffer().len(),
            })
        }
        ControlSocketRead::Eof => Ok(ControlConnectionReadTurn::Eof),
        ControlSocketRead::WouldBlock => Ok(ControlConnectionReadTurn::WouldBlock {
            buffered_bytes: state.read_buffer().len(),
        }),
    }
}

pub fn flush_control_connection<I>(
    io: &mut I,
    state: &mut ControlConnectionState,
) -> Result<ControlConnectionWriteTurn, ControlConnectionWriteTurnError>
where
    I: ControlConnectionIo + ?Sized,
{
    if state.write_buffer().is_empty() {
        return Ok(ControlConnectionWriteTurn::Idle {
            close_after_write: state.close_after_write(),
        });
    }

    let pending_before = state.write_buffer().pending_len();
    match io
        .write_control(state.write_buffer().pending_slice())
        .map_err(ControlConnectionWriteTurnError::Socket)?
    {
        ControlSocketWrite::Complete => {
            state
                .write_buffer_mut()
                .advance_written(pending_before)
                .map_err(ControlConnectionWriteTurnError::Buffer)?;
            Ok(ControlConnectionWriteTurn::Complete {
                written: pending_before,
                close_after_write: state.close_after_write(),
            })
        }
        ControlSocketWrite::Partial { written } => {
            state
                .write_buffer_mut()
                .advance_written(written)
                .map_err(ControlConnectionWriteTurnError::Buffer)?;
            Ok(ControlConnectionWriteTurn::Partial {
                written,
                pending_bytes: state.write_buffer().pending_len(),
            })
        }
        ControlSocketWrite::WouldBlock => Ok(ControlConnectionWriteTurn::WouldBlock {
            pending_bytes: pending_before,
        }),
    }
}
