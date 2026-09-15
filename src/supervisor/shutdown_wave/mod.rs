mod deadline;
mod target;

pub(super) use deadline::service_root_cgroup_id;

use crate::boundary::{ProcessController, ProcessSignal};
use crate::service::runtime::{
    ServiceState, ServiceStoppingTimeoutEvidence, ServiceTransition, TransitionCause,
};
use crate::shutdown::{ShutdownError, ShutdownPlan, ShutdownStopDeadline};

use super::dispatch::SupervisorShutdownStopDispatch;
use super::shutdown_progress::service_done_for_shutdown;
use super::work::SupervisorWork;

use deadline::{retained_stop_deadline, stop_deadline_ns};
use target::process_target;

pub(super) fn begin_first_stop_wave<P>(
    work: &mut SupervisorWork,
    plan: &ShutdownPlan,
    controller: &mut P,
    now_ns: u64,
    stop_deadlines: &mut Vec<ShutdownStopDeadline>,
) -> Result<Vec<SupervisorShutdownStopDispatch>, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    begin_stop_wave(work, plan, 0, controller, now_ns, stop_deadlines)
}

pub(super) fn begin_stop_wave<P>(
    work: &mut SupervisorWork,
    plan: &ShutdownPlan,
    wave_index: usize,
    controller: &mut P,
    now_ns: u64,
    stop_deadlines: &mut Vec<ShutdownStopDeadline>,
) -> Result<Vec<SupervisorShutdownStopDispatch>, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let Some(wave) = plan.stop_waves.get(wave_index) else {
        return Ok(Vec::new());
    };
    let mut dispatches = Vec::with_capacity(wave.services.len());

    for participant in &wave.services {
        // The plan was fixed when the shutdown began, and a participant can
        // leave the running states before its wave comes round — crash, or
        // exit cleanly, while an earlier wave was still draining. There is
        // nothing left to stop: no process to signal, no deadline to hold
        // the wave open for. Skipped here by the same test `wave_complete`
        // applies, so the two agree on who is still in the wave. Asking
        // `process_target` for its process instead raised
        // MissingRunningService, and that ended PID 1's runtime loop in the
        // middle of the shutdown (PEI-1086).
        if participant_done(work, &participant.service) {
            continue;
        }
        if participant.already_stopping {
            let retained = retained_stop_deadline(work, &participant.service, wave_index, now_ns)?;
            stop_deadlines.push(retained.deadline.clone());
            dispatches.push(SupervisorShutdownStopDispatch {
                service: participant.service.clone(),
                already_stopping: true,
                target: None,
                signal: None,
                service_transition: None,
                deadline: Some(retained.deadline),
                unsubstantiated_deadline: retained.unsubstantiated,
            });
            continue;
        }

        let target = process_target(&work.jobs, &participant.service)?;
        let stopping_acknowledged = work
            .services
            .runtime(&participant.service)
            .is_some_and(|runtime| runtime.stopping_acknowledged);
        if !stopping_acknowledged {
            controller
                .signal_main(&target, ProcessSignal::Sigterm)
                .map_err(ShutdownError::Boundary)?;
        }
        let stop_cause = TransitionCause::ShutdownWave;
        let service_transition = work
            .services
            .transition_service(
                &participant.service,
                ServiceTransition {
                    to: ServiceState::Stopping,
                    cause: stop_cause,
                },
            )
            .map_err(ShutdownError::ServiceTable)?;
        let due_at_ns = stop_deadline_ns(&work.services, &participant.service, now_ns)?;
        work.services
            .record_stopping_timeout(
                &participant.service,
                ServiceStoppingTimeoutEvidence {
                    started_at_ns: now_ns,
                    due_at_ns,
                    cause: stop_cause,
                },
            )
            .map_err(ShutdownError::ServiceTable)?;
        let deadline = ShutdownStopDeadline {
            service: participant.service.clone(),
            cgroup_id: service_root_cgroup_id(&work.services, &participant.service)?,
            started_at_ns: now_ns,
            due_at_ns,
            wave: wave_index,
            operation_id: None,
        };
        stop_deadlines.push(deadline.clone());
        dispatches.push(SupervisorShutdownStopDispatch {
            service: participant.service.clone(),
            already_stopping: false,
            target: Some(target),
            signal: (!stopping_acknowledged).then_some(ProcessSignal::Sigterm),
            service_transition: Some(service_transition),
            deadline: Some(deadline),
            unsubstantiated_deadline: None,
        });
    }

    Ok(dispatches)
}

fn participant_done(work: &SupervisorWork, service: &str) -> bool {
    work.services
        .runtime(service)
        .is_some_and(|runtime| service_done_for_shutdown(runtime.state))
}
