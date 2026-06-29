use std::collections::VecDeque;
use std::io;

use crate::control::connection::ControlConnectionIo;
use crate::control::socket::{
    ControlSocketRead, ControlSocketReadError, ControlSocketWrite, ControlSocketWriteError,
};
use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessCheckError,
    SystemAccessCheckRequest, SystemAccessChecker, SystemAccessDecision,
};
use crate::security::TokenSummary;
use crate::supervisor::tests::{ScriptedClock, TestProcessController};
use crate::supervisor::{
    SupervisorShutdownControlConnectionTurnContext, SupervisorShutdownControlFrameContext,
};

pub(super) static DEFAULT_CONTROL_SECURITY: ControlSecurityDescriptor =
    ControlSecurityDescriptor::Default;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TurnAccessCall {
    pub token_fd: i32,
    pub descriptor: ControlSecurityDescriptor,
    pub desired_access: SystemAccess,
}

#[derive(Debug)]
pub(super) struct TurnAccessChecker {
    decision: SystemAccessDecision,
    pub calls: Vec<TurnAccessCall>,
}

impl TurnAccessChecker {
    pub fn allow() -> Self {
        Self {
            decision: SystemAccessDecision {
                allowed: true,
                granted_access_bits: SystemAccess::ALL.bits(),
            },
            calls: Vec::new(),
        }
    }

    pub fn deny() -> Self {
        Self {
            decision: SystemAccessDecision {
                allowed: false,
                granted_access_bits: 0,
            },
            calls: Vec::new(),
        }
    }
}

impl SystemAccessChecker for TurnAccessChecker {
    fn check_system_access(
        &mut self,
        request: SystemAccessCheckRequest<'_>,
    ) -> Result<SystemAccessDecision, SystemAccessCheckError> {
        self.calls.push(TurnAccessCall {
            token_fd: request.token_fd,
            descriptor: request.descriptor.clone(),
            desired_access: request.desired_access,
        });
        Ok(self.decision)
    }
}

pub(super) fn control_peer() -> ControlPeer {
    ControlPeer::borrowed_token_fd(44, TokenSummary::requested_identity("admin"))
}

pub(super) fn turn_context<'a>(
    peer: &'a ControlPeer,
    access_checker: &'a mut TurnAccessChecker,
    controller: &'a mut TestProcessController,
    clock: &'a mut ScriptedClock,
    max_request_bytes: usize,
) -> SupervisorShutdownControlFrameContext<
    'a,
    ScriptedClock,
    TestProcessController,
    TurnAccessChecker,
> {
    SupervisorShutdownControlFrameContext {
        peer,
        control_security: &DEFAULT_CONTROL_SECURITY,
        access_checker,
        controller,
        clock,
        max_request_bytes,
    }
}

pub(super) fn connection_turn_context<'a>(
    access_checker: &'a mut TurnAccessChecker,
    controller: &'a mut TestProcessController,
    clock: &'a mut ScriptedClock,
    max_read_bytes: usize,
    max_request_bytes: usize,
) -> SupervisorShutdownControlConnectionTurnContext<
    'a,
    ScriptedClock,
    TestProcessController,
    TurnAccessChecker,
> {
    SupervisorShutdownControlConnectionTurnContext {
        control_security: &DEFAULT_CONTROL_SECURITY,
        access_checker,
        controller,
        clock,
        max_read_bytes,
        max_request_bytes,
        observed_at_ns: 123,
    }
}

pub(super) fn assert_response(
    line: &[u8],
    status: &str,
    code: Option<&str>,
    message: Option<&str>,
) {
    assert_eq!(line.last(), Some(&b'\n'));
    let response: serde_json::Value =
        serde_json::from_slice(&line[..line.len() - 1]).expect("response json");
    assert_eq!(response["status"], status);
    match code {
        Some(code) => assert_eq!(response["code"], code),
        None => assert!(response.get("code").is_none()),
    }
    match message {
        Some(message) => assert_eq!(response["message"], message),
        None => assert!(response.get("message").is_none()),
    }
}

#[derive(Debug, Default)]
pub(super) struct FakeConnectionIo {
    reads: VecDeque<ControlSocketRead>,
    pub writes: Vec<Vec<u8>>,
    write_results: VecDeque<ControlSocketWrite>,
}

impl FakeConnectionIo {
    pub fn scripted(
        reads: impl IntoIterator<Item = ControlSocketRead>,
        writes: impl IntoIterator<Item = ControlSocketWrite>,
    ) -> Self {
        Self {
            reads: reads.into_iter().collect(),
            writes: Vec::new(),
            write_results: writes.into_iter().collect(),
        }
    }
}

impl ControlConnectionIo for FakeConnectionIo {
    fn read_control(
        &mut self,
        _max_bytes: usize,
    ) -> Result<ControlSocketRead, ControlSocketReadError> {
        Ok(self
            .reads
            .pop_front()
            .unwrap_or(ControlSocketRead::WouldBlock))
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
