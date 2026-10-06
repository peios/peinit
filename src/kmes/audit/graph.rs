use crate::boot::phase2::{BlockedReason, SafeModeDowngrade};
use crate::boundary::{BoundaryError, KmesEvent};
use crate::service::{ServiceDependencyKind, ServiceGraphFinding, ServiceGraphWarning};

use crate::kmes::labels::{service_dependency_kind_label, service_type_label};
use crate::kmes::payload::Payload;
use crate::kmes::types::{BOOT_DOWNGRADED, GRAPH_VALIDATION_FAILED, GRAPH_VALIDATION_WARNED};

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// When the service graph was validated (`graph.phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphPhase {
    /// The boot's own plan: findings mark services and the boot continues.
    Boot,
    /// A `reload-config`, explicit or watch-triggered: a finding rejects the
    /// whole reload.
    ReloadConfig,
    /// The phase-2 boot plan could not be built.
    Phase2Boot,
}

impl GraphPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::ReloadConfig => "reload-config",
            Self::Phase2Boot => "phase2-boot",
        }
    }
}

fn validation(phase: GraphPhase, reason: &'static str) -> Payload {
    let mut payload = Payload::new();
    payload.set("graph.phase", phase.label());
    payload.set("outcome.reason", reason);
    payload
}

/// `peinit.graph.validation.warned`: the graph passed, with a warning.
pub fn encode_graph_validation_warning_event(
    phase: GraphPhase,
    warning: &ServiceGraphWarning,
) -> Result<KmesEvent, BoundaryError> {
    let payload = match warning {
        ServiceGraphWarning::AliveReadinessWithHardDependents {
            service,
            dependents,
        } => {
            let mut payload = validation(phase, "alive-readiness-with-hard-dependents");
            payload.set("object.service.name", service.as_str());
            payload.set("object.service.dependents", dependents.as_slice());
            payload
        }
        ServiceGraphWarning::UnfilledRole { role, services } => {
            let mut payload = validation(phase, "unfilled-role");
            payload.set("graph.role", role.as_str());
            payload.set("graph.services", services.as_slice());
            payload
        }
    };
    payload.finish(GRAPH_VALIDATION_WARNED)
}

/// `peinit.graph.validation.failed`, one per finding.
pub fn encode_graph_validation_error_event(
    phase: GraphPhase,
    finding: &ServiceGraphFinding,
) -> Result<KmesEvent, BoundaryError> {
    match finding {
        ServiceGraphFinding::InvalidServiceName { service } => {
            encode_graph_service_error(phase, "invalid-service-name", service)
        }
        ServiceGraphFinding::DuplicateService { service } => {
            encode_graph_service_error(phase, "duplicate-service", service)
        }
        ServiceGraphFinding::MissingHardDependency {
            service,
            target,
            kind,
        } => encode_dependency_error(phase, "missing-hard-dependency", service, target, *kind),
        ServiceGraphFinding::Cycle { services } => encode_graph_cycle_error(phase, services),
        ServiceGraphFinding::ConflictingBootServices { service, target } => {
            encode_conflict_error(phase, service, target)
        }
        ServiceGraphFinding::InvalidHealthCheckRestartWindow {
            service,
            retries,
            interval_secs,
            restart_window_secs,
        } => {
            let mut payload = validation(phase, "invalid-health-check-restart-window");
            payload.set("object.service.name", service.as_str());
            payload.set(
                "object.service.health-check.interval",
                interval_secs.saturating_mul(NANOS_PER_SECOND),
            );
            payload.set("object.service.health-check.retries", u64::from(*retries));
            payload.set(
                "object.service.health-check.restart-window",
                restart_window_secs.saturating_mul(NANOS_PER_SECOND),
            );
            payload.finish(GRAPH_VALIDATION_FAILED)
        }
        ServiceGraphFinding::UnschedulableHealthCheck {
            service,
            service_type,
        } => {
            let mut payload = validation(phase, "unschedulable-health-check");
            payload.set("object.service.name", service.as_str());
            payload.set("object.service.type", service_type_label(*service_type));
            payload.finish(GRAPH_VALIDATION_FAILED)
        }
        ServiceGraphFinding::InvalidTimerSchedule {
            service,
            schedule,
            message,
        } => {
            let mut payload = validation(phase, "invalid-timer-schedule");
            payload.set("object.service.name", service.as_str());
            payload.set("object.service.timer.schedule", schedule.as_str());
            payload.set("outcome.detail", message.as_str());
            payload.finish(GRAPH_VALIDATION_FAILED)
        }
    }
}

pub(super) fn encode_graph_service_error(
    phase: GraphPhase,
    finding: &'static str,
    service: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = validation(phase, finding);
    payload.set("object.service.name", service);
    payload.finish(GRAPH_VALIDATION_FAILED)
}

pub(super) fn encode_graph_cycle_error(
    phase: GraphPhase,
    services: &[String],
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = validation(phase, "cycle");
    payload.set("graph.services", services);
    payload.finish(GRAPH_VALIDATION_FAILED)
}

fn encode_conflict_error(
    phase: GraphPhase,
    service: &str,
    target: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = validation(phase, "conflicting-boot-services");
    payload.set("object.service.name", service);
    payload.set("object.service.conflict.name", target);
    payload.finish(GRAPH_VALIDATION_FAILED)
}

fn encode_dependency_error(
    phase: GraphPhase,
    finding: &'static str,
    service: &str,
    target: &str,
    kind: ServiceDependencyKind,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = validation(phase, finding);
    payload.set("object.service.name", service);
    payload.set("object.service.dependency.name", target);
    payload.set(
        "object.service.dependency.kind",
        service_dependency_kind_label(kind),
    );
    payload.finish(GRAPH_VALIDATION_FAILED)
}

/// One boot-time blocked-service finding, as a
/// `peinit.graph.validation.failed`.
///
/// Deliberately the same event type as the reload path's findings, told
/// apart by `graph.phase` (`boot` here, `reload-config` there). A consumer
/// filtering for validation problems then gets both without knowing two
/// type names, and the phase tells it which regime it is looking at — boot
/// marks individual services and continues, reload rejects the whole
/// reload.
///
/// `BlockedReason` is not `ServiceGraphFinding` and does not map onto it one
/// for one: `HardDependencyBlocked` — blocked *because a dependency is
/// blocked* — is real information the reload path cannot produce, since
/// reload rejects wholesale rather than propagating a block. It gets its own
/// reason rather than being flattened into the missing-dependency case,
/// which would claim the dependency does not exist when it does.
pub fn encode_boot_blocked_service_event(
    service: &str,
    reason: &BlockedReason,
) -> Result<KmesEvent, BoundaryError> {
    encode_blocked_service_event(GraphPhase::Boot, service, reason)
}

/// A key a reload found but could not decode: the same `validation-error`
/// finding the boot records for one, under the `reload-config` phase, so a
/// consumer sees the service fail the same way whichever path read it
/// (PEI-621).
pub fn encode_reload_undecodable_service_event(
    service: &crate::boundary::UndecodableService,
) -> Result<KmesEvent, BoundaryError> {
    encode_blocked_service_event(
        GraphPhase::ReloadConfig,
        &service.name,
        &BlockedReason::ValidationError {
            message: crate::service::undecodable_message(service),
        },
    )
}

fn encode_blocked_service_event(
    phase: GraphPhase,
    service: &str,
    reason: &BlockedReason,
) -> Result<KmesEvent, BoundaryError> {
    match reason {
        BlockedReason::CycleDetected { services } => encode_graph_cycle_error(phase, services),
        BlockedReason::HardDependencyUnavailable { target, kind } => {
            encode_dependency_error(phase, "missing-hard-dependency", service, target, *kind)
        }
        BlockedReason::HardDependencyBlocked { target, kind } => {
            encode_dependency_error(phase, "hard-dependency-blocked", service, target, *kind)
        }
        BlockedReason::ConflictingBootService { target } => {
            encode_conflict_error(phase, service, target)
        }
        BlockedReason::ValidationError { message } => {
            let mut payload = validation(phase, "validation-error");
            payload.set("object.service.name", service);
            payload.set("outcome.detail", message.as_str());
            payload.finish(GRAPH_VALIDATION_FAILED)
        }
    }
}

/// `peinit.boot.downgraded`: why a Full boot was downgraded to Safe mode.
///
/// A distinct event type from `peinit.graph.validation.failed`, because it
/// is a different claim: not "this service is broken" but "the machine is
/// running a reduced service set, and here is what forced that". The
/// services named are deliberately not marked Failed — Safe mode was never
/// going to start them — so this event is the only record that they were
/// the cause.
pub fn encode_safe_mode_downgrade_event(
    downgrade: &SafeModeDowngrade,
) -> Result<KmesEvent, BoundaryError> {
    let mut payload = Payload::new();
    match downgrade {
        SafeModeDowngrade::CriticalCycle { services } => {
            payload.set("outcome.reason", "critical-cycle");
            payload.set("graph.services", services.as_slice());
        }
        SafeModeDowngrade::CriticalBootConflict { service, target } => {
            payload.set("outcome.reason", "critical-boot-conflict");
            payload.set("object.service.name", service.as_str());
            payload.set("object.service.conflict.name", target.as_str());
        }
    }
    payload.finish(BOOT_DOWNGRADED)
}
