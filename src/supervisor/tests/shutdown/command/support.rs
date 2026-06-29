use std::collections::VecDeque;

use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessCheckError,
    SystemAccessCheckRequest, SystemAccessChecker, SystemAccessDecision,
};
use crate::security::TokenSummary;
use crate::shutdown::ShutdownKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CommandFinalizerCall {
    SnapshotMounts,
    RemountRootReadonly,
    Sync,
    Reboot(ShutdownKind),
}

#[derive(Debug, Default)]
pub(super) struct CommandFinalizer {
    pub(super) calls: Vec<CommandFinalizerCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FakeAccessCall {
    pub(super) token_fd: i32,
    pub(super) descriptor: ControlSecurityDescriptor,
    pub(super) desired_access: SystemAccess,
}

#[derive(Debug)]
pub(super) struct FakeSystemAccessChecker {
    decisions: VecDeque<Result<SystemAccessDecision, SystemAccessCheckError>>,
    pub(super) calls: Vec<FakeAccessCall>,
}

impl FakeSystemAccessChecker {
    pub(super) fn allow(granted_access_bits: u32) -> Self {
        Self::from_decision(Ok(SystemAccessDecision {
            allowed: true,
            granted_access_bits,
        }))
    }

    pub(super) fn deny(granted_access_bits: u32) -> Self {
        Self::from_decision(Ok(SystemAccessDecision {
            allowed: false,
            granted_access_bits,
        }))
    }

    fn from_decision(decision: Result<SystemAccessDecision, SystemAccessCheckError>) -> Self {
        Self {
            decisions: VecDeque::from([decision]),
            calls: Vec::new(),
        }
    }
}

impl SystemAccessChecker for FakeSystemAccessChecker {
    fn check_system_access(
        &mut self,
        request: SystemAccessCheckRequest<'_>,
    ) -> Result<SystemAccessDecision, SystemAccessCheckError> {
        self.calls.push(FakeAccessCall {
            token_fd: request.token_fd,
            descriptor: request.descriptor.clone(),
            desired_access: request.desired_access,
        });
        self.decisions
            .pop_front()
            .unwrap_or(Ok(SystemAccessDecision {
                allowed: true,
                granted_access_bits: SystemAccess::ALL.bits(),
            }))
    }
}

pub(super) fn control_peer() -> ControlPeer {
    ControlPeer::borrowed_token_fd(44, TokenSummary::requested_identity("admin"))
}

impl ShutdownFinalizer for CommandFinalizer {
    fn snapshot_mounts(&mut self) -> Result<Vec<String>, BoundaryError> {
        self.calls.push(CommandFinalizerCall::SnapshotMounts);
        Ok(vec!["/".to_string()])
    }

    fn unmount(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        panic!("unexpected non-root mount cleanup for {mount_point}");
    }

    fn remount_readonly(&mut self, mount_point: &str) -> Result<(), BoundaryError> {
        assert_eq!(mount_point, "/");
        self.calls.push(CommandFinalizerCall::RemountRootReadonly);
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(CommandFinalizerCall::Sync);
        Ok(())
    }

    fn reboot(&mut self, kind: ShutdownKind) -> Result<(), BoundaryError> {
        self.calls.push(CommandFinalizerCall::Reboot(kind));
        Ok(())
    }
}
