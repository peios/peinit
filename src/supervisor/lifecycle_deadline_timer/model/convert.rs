use crate::execution::control::{
    ReloadCommandDeadline, ReloadDetectionDeadline, StopTimeoutDeadline,
};
use crate::execution::start::{
    PostStartHookDeadline, PreStartCheckDeadline, PreStartHookDeadline, ReadinessDeadline,
};
use crate::service::RestartBackoffDeadline;
use crate::supervisor::boot_success::BootSuccessDeadline;
use crate::supervisor::health::{HealthCheckIntervalDeadline, HealthCheckTimeoutDeadline};
use crate::supervisor::watchdog::WatchdogDeadline;

use super::{SupervisorLifecycleDeadline, SupervisorLifecycleDeadlineKind};

impl From<PreStartCheckDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: PreStartCheckDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::PreStartCheckTimeout {
                service: deadline.service,
                operation_id: deadline.operation_id,
                result_fd: deadline.result_fd,
                pidfd: deadline.pidfd,
            },
        }
    }
}

impl From<PreStartHookDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: PreStartHookDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::PreStartHookTimeout {
                service: deadline.service,
                operation_id: deadline.operation_id,
                job_id: deadline.job_id,
            },
        }
    }
}

impl From<PostStartHookDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: PostStartHookDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::PostStartHookTimeout {
                service: deadline.service,
                operation_id: deadline.operation_id,
                job_id: deadline.job_id,
            },
        }
    }
}

impl From<ReadinessDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: ReadinessDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::ReadinessTimeout {
                service: deadline.service,
                operation_id: deadline.operation_id,
                job_id: deadline.job_id,
            },
        }
    }
}

impl From<StopTimeoutDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: StopTimeoutDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::StopTimeout {
                service: deadline.service,
                operation_id: deadline.operation_id,
            },
        }
    }
}

impl From<ReloadDetectionDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: ReloadDetectionDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::ReloadDetection {
                service: deadline.service,
                operation_id: deadline.operation_id,
            },
        }
    }
}

impl From<ReloadCommandDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: ReloadCommandDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::ReloadCommandTimeout {
                service: deadline.service,
                operation_id: deadline.operation_id,
                job_id: deadline.job_id,
            },
        }
    }
}

impl From<RestartBackoffDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: RestartBackoffDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::RestartBackoff {
                service: deadline.service,
            },
        }
    }
}

impl From<HealthCheckIntervalDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: HealthCheckIntervalDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::HealthCheckInterval {
                service: deadline.service,
                generation: deadline.activation_generation,
            },
        }
    }
}

impl From<HealthCheckTimeoutDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: HealthCheckTimeoutDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::HealthCheckTimeout {
                service: deadline.service,
                generation: deadline.activation_generation,
                job_id: deadline.job_id,
            },
        }
    }
}

impl From<WatchdogDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: WatchdogDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::WatchdogTimeout {
                service: deadline.service,
                generation: deadline.generation,
            },
        }
    }
}

impl From<BootSuccessDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: BootSuccessDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::BootSuccess,
        }
    }
}

impl From<crate::supervisor::cgroup_cleanup::CgroupCleanupDeadline>
    for SupervisorLifecycleDeadline
{
    fn from(deadline: crate::supervisor::cgroup_cleanup::CgroupCleanupDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::CgroupCleanup {
                service: deadline.service,
                cgroup_id: deadline.cgroup_id,
            },
        }
    }
}
