use crate::control::system::ControlPeer;

use super::{
    ControlConnectionIo, ControlConnectionReadTurn, ControlConnectionState,
    ControlConnectionWriteTurn, ControlConnectionWriteTurnError, flush_control_connection,
    read_control_connection,
};

#[derive(Debug)]
pub struct ControlConnectionRecord<I> {
    io: I,
    peer: ControlPeer,
    state: ControlConnectionState,
}

impl<I> ControlConnectionRecord<I> {
    pub fn new(io: I, peer: ControlPeer) -> Self {
        Self::new_with_activity(io, peer, None)
    }

    pub fn new_with_activity(io: I, peer: ControlPeer, observed_at_ns: Option<u64>) -> Self {
        let mut state = ControlConnectionState::new();
        if let Some(observed_at_ns) = observed_at_ns {
            state.mark_activity(observed_at_ns);
        }
        Self { io, peer, state }
    }

    pub fn peer(&self) -> &ControlPeer {
        &self.peer
    }

    pub fn peer_and_state_mut(&mut self) -> (&ControlPeer, &mut ControlConnectionState) {
        let Self { peer, state, .. } = self;
        (peer, state)
    }

    pub fn state(&self) -> &ControlConnectionState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut ControlConnectionState {
        &mut self.state
    }

    pub fn io(&self) -> &I {
        &self.io
    }

    pub fn io_mut(&mut self) -> &mut I {
        &mut self.io
    }
}

impl<I> ControlConnectionRecord<I>
where
    I: ControlConnectionIo,
{
    pub fn read(
        &mut self,
        max_read_bytes: usize,
    ) -> Result<ControlConnectionReadTurn, crate::control::socket::ControlSocketReadError> {
        read_control_connection(&mut self.io, &mut self.state, max_read_bytes)
    }

    pub fn flush(&mut self) -> Result<ControlConnectionWriteTurn, ControlConnectionWriteTurnError> {
        flush_control_connection(&mut self.io, &mut self.state)
    }
}
