use crate::boundary::{ProcessController, ShutdownFinalizer};
use crate::job::service_cgroup_root_path;
use crate::service::runtime::ProcessPresence;
use crate::shutdown::{
    ShutdownError, ShutdownFinalizationState, ShutdownKind, ShutdownPlan, ShutdownRuntime,
};

use super::dispatch::{SupervisorImmediateShutdownDispatch, SupervisorShutdownCgroupKillDispatch};
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    /// Kill every service outright and reboot: the third SIGINT.
    ///
    /// Given no finalizer, the kills are done and the shutdown is installed
    /// as an immediate final action already due, for the caller to finalise
    /// once it has said what it needs to say: the runtime writes the turn's
    /// console output first, because `reboot(2)` does not return (PEI-827).
    pub fn force_reboot_shutdown<P>(
        &mut self,
        controller: &mut P,
        finalizer: Option<&mut dyn ShutdownFinalizer>,
        now_ns: u64,
    ) -> Result<SupervisorImmediateShutdownDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        let killed_services = kill_all_process_cgroups(&mut work, controller, now_ns)
            .map_err(SupervisorError::Shutdown)?;
        work.shutdown = Some(immediate_runtime(ShutdownKind::Reboot, now_ns));
        work.commit(self);

        let finalization = match finalizer {
            Some(finalizer) => Some(self.finalize_without_mount_cleanup(finalizer, now_ns)?),
            None => None,
        };
        Ok(SupervisorImmediateShutdownDispatch {
            killed_services,
            finalization,
        })
    }

    pub fn critical_reboot<F>(
        &mut self,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<super::dispatch::SupervisorShutdownFinalizationDispatch, SupervisorError>
    where
        F: ShutdownFinalizer + ?Sized,
    {
        self.shutdown = Some(immediate_runtime(ShutdownKind::Reboot, now_ns));
        self.finalize_without_mount_cleanup(finalizer, now_ns)
    }
}

fn kill_all_process_cgroups<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    now_ns: u64,
) -> Result<Vec<SupervisorShutdownCgroupKillDispatch>, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let services = work
        .services
        .service_names()
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut killed = Vec::new();

    for service in services {
        let Some(runtime) = work.services.runtime(&service) else {
            continue;
        };
        if runtime.state.process_presence() == ProcessPresence::None {
            continue;
        }
        let cgroup_id = service_cgroup_root_path(&service, runtime.cgroup_generation);
        controller
            .kill_cgroup(&cgroup_id)
            .map_err(ShutdownError::Boundary)?;
        killed.push(SupervisorShutdownCgroupKillDispatch {
            service,
            cgroup_id,
            killed_at_ns: now_ns,
        });
    }

    Ok(killed)
}

fn immediate_runtime(kind: ShutdownKind, now_ns: u64) -> ShutdownRuntime {
    ShutdownRuntime {
        kind,
        initiated_at_ns: now_ns,
        global_deadline_ns: now_ns,
        plan: ShutdownPlan {
            completed_to_clear: Vec::new(),
            starting_to_kill: Vec::new(),
            stop_waves: Vec::new(),
            ignored: Vec::new(),
        },
        current_wave: 0,
        stop_deadlines: Vec::new(),
        post_kill_deadlines: Vec::new(),
        finalization: ShutdownFinalizationState::Failed {
            message: "immediate final action not attempted yet".to_string(),
            next_retry_at_ns: now_ns,
        },
    }
}
