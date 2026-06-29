use std::collections::{BTreeSet, VecDeque};
use std::io;

use crate::control::connection::ControlConnectionIo;
use crate::control::service_security::{
    ServiceAccess, ServiceAccessCheckError, ServiceAccessCheckRequest, ServiceAccessChecker,
    ServiceAccessDecision,
};
use crate::control::socket::{
    ControlSocketRead, ControlSocketReadError, ControlSocketWrite, ControlSocketWriteError,
};
use crate::control::system::{
    ControlPeer, ControlSecurityDescriptor, SystemAccess, SystemAccessCheckError,
    SystemAccessCheckRequest, SystemAccessChecker, SystemAccessDecision,
};
use crate::security::TokenSummary;
use crate::service::{Readiness, ServiceDefinition};
use crate::supervisor::{Supervisor, SupervisorControlCommandBodyContext, SupervisorSettings};

use super::super::{BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController, settings};

pub(super) static DEFAULT_CONTROL_SECURITY: ControlSecurityDescriptor =
    ControlSecurityDescriptor::Default;

pub(super) fn booted_supervisor(services: Vec<ServiceDefinition>) -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(services);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
}

pub(super) fn inactive_alive_service(name: &str) -> ServiceDefinition {
    let mut definition = ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"));
    definition.triggers.clear();
    definition.readiness = Readiness::Alive;
    definition
}

pub(super) fn body_context<'a>(
    peer: &'a ControlPeer,
    access_checker: &'a mut TestAccessChecker,
    controller: &'a mut TestProcessController,
    clock: &'a mut ScriptedClock,
) -> SupervisorControlCommandBodyContext<
    'a,
    'a,
    ScriptedClock,
    TestProcessController,
    TestAccessChecker,
> {
    SupervisorControlCommandBodyContext {
        peer,
        control_security: &DEFAULT_CONTROL_SECURITY,
        access_checker,
        controller,
        clock,
        registry: None,
    }
}

pub(super) fn control_peer() -> ControlPeer {
    ControlPeer::borrowed_token_fd(44, TokenSummary::requested_identity("admin"))
}

pub(super) fn response_json(line: &[u8]) -> serde_json::Value {
    assert_eq!(line.last(), Some(&b'\n'));
    serde_json::from_slice(&line[..line.len() - 1]).expect("response json")
}

#[derive(Debug)]
pub(super) struct TestAccessChecker {
    allowed_services: Option<BTreeSet<String>>,
    system_allowed: bool,
}

impl TestAccessChecker {
    pub(super) fn allow_all() -> Self {
        Self {
            allowed_services: None,
            system_allowed: true,
        }
    }

    pub(super) fn deny_all_services() -> Self {
        Self {
            allowed_services: Some(BTreeSet::new()),
            system_allowed: true,
        }
    }

    pub(super) fn allow_only_services<const N: usize>(services: [&str; N]) -> Self {
        Self {
            allowed_services: Some(services.into_iter().map(ToString::to_string).collect()),
            system_allowed: true,
        }
    }

    pub(super) fn deny_system() -> Self {
        Self {
            allowed_services: None,
            system_allowed: false,
        }
    }
}

impl SystemAccessChecker for TestAccessChecker {
    fn check_system_access(
        &mut self,
        _request: SystemAccessCheckRequest<'_>,
    ) -> Result<SystemAccessDecision, SystemAccessCheckError> {
        Ok(SystemAccessDecision {
            allowed: self.system_allowed,
            granted_access_bits: if self.system_allowed {
                SystemAccess::ALL.bits()
            } else {
                0
            },
        })
    }
}

impl ServiceAccessChecker for TestAccessChecker {
    fn check_service_access(
        &mut self,
        request: ServiceAccessCheckRequest<'_>,
    ) -> Result<ServiceAccessDecision, ServiceAccessCheckError> {
        let allowed = self
            .allowed_services
            .as_ref()
            .is_none_or(|services| services.contains(request.service));
        Ok(ServiceAccessDecision {
            allowed,
            granted_access_bits: if allowed {
                ServiceAccess::ALL.bits()
            } else {
                0
            },
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct FakeConnectionIo {
    reads: VecDeque<ControlSocketRead>,
    pub(super) writes: std::cell::RefCell<Vec<Vec<u8>>>,
}

impl FakeConnectionIo {
    pub(super) fn scripted_reads(reads: impl IntoIterator<Item = ControlSocketRead>) -> Self {
        Self {
            reads: reads.into_iter().collect(),
            writes: std::cell::RefCell::new(Vec::new()),
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
        self.writes.borrow_mut().push(bytes.to_vec());
        if bytes.is_empty() {
            Err(ControlSocketWriteError::Write(io::Error::new(
                io::ErrorKind::WriteZero,
                "empty write",
            )))
        } else {
            Ok(ControlSocketWrite::Complete)
        }
    }
}
