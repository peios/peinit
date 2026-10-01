use crate::boundary::ProcessController;
use crate::service::runtime::ServiceState;
use crate::shutdown::{ShutdownError, ShutdownFinalizationState, ShutdownRuntime};

use super::dispatch::SupervisorShutdownStopDispatch;
use super::shutdown_wave::begin_stop_wave;
use super::state::SupervisorError;
use super::submitted::live_submitted_jobs_remain;
use super::work::SupervisorWork;

pub(super) fn ensure_shutdown(work: &SupervisorWork) -> Result<(), SupervisorError> {
    work.shutdown()
        .map(|_| ())
        .map_err(SupervisorError::Shutdown)
}

pub(in crate::supervisor) fn advance_shutdown_progress<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    now_ns: u64,
) -> Result<Vec<SupervisorShutdownStopDispatch>, ShutdownError>
where
    P: ProcessController + ?Sized,
{
    let mut shutdown = work
        .shutdown
        .take()
        .ok_or(ShutdownError::NoShutdownInProgress)?;
    let mut dispatches = Vec::new();

    loop {
        if !current_wave_complete(work, &shutdown) {
            break;
        }
        let next_wave = shutdown.current_wave.saturating_add(1);
        if next_wave >= shutdown.plan.stop_waves.len() {
            // Only a shutdown still waiting for its services becomes ready.
            // A forced reboot installs an empty plan already in the Failed
            // finalisation state, and the reaps of the services it SIGKILLed
            // arrive here afterwards; overwriting Failed with Ready made the
            // next drive run the full graceful finalisation — seed write,
            // unmounts, remounts — instead of retrying sync and reboot, the
            // action that failed (PEI-1087).
            if shutdown.stop_deadlines.is_empty()
                && shutdown.post_kill_deadlines.is_empty()
                && !live_submitted_jobs_remain(work)
                && shutdown.finalization == ShutdownFinalizationState::WaitingForServices
            {
                shutdown.finalization = ShutdownFinalizationState::Ready;
            }
            break;
        }

        shutdown.current_wave = next_wave;
        if wave_complete(work, &shutdown, next_wave) {
            continue;
        }
        let mut next_deadlines = std::mem::take(&mut shutdown.stop_deadlines);
        let mut wave_dispatches = begin_stop_wave(
            work,
            &shutdown.plan,
            next_wave,
            controller,
            now_ns,
            &mut next_deadlines,
        )?;
        shutdown.stop_deadlines = next_deadlines;
        dispatches.append(&mut wave_dispatches);
    }

    work.shutdown = Some(shutdown);
    Ok(dispatches)
}

/// Everything that keeps the shutdown from finalizing, for the console: the
/// participants not yet stopped (with their state, or that the service table
/// no longer knows them), the stop and post-kill checks still pending, and the
/// submitted jobs still live (PEI-1216).
pub(in crate::supervisor) fn shutdown_waiting_for(work: &SupervisorWork) -> Vec<String> {
    let Some(shutdown) = &work.shutdown else {
        return Vec::new();
    };
    let mut waiting = Vec::new();
    for (index, wave) in shutdown.plan.stop_waves.iter().enumerate() {
        for participant in &wave.services {
            if let Some(runtime) = work.services.runtime(&participant.service)
                && !service_done_for_shutdown(runtime.state)
            {
                waiting.push(format!(
                    "{} ({:?}, wave {index})",
                    participant.service, runtime.state
                ));
            }
        }
    }
    for deadline in &shutdown.stop_deadlines {
        waiting.push(format!("stop timeout of {}", deadline.service));
    }
    for deadline in &shutdown.post_kill_deadlines {
        waiting.push(format!("post-kill check of {}", deadline.service));
    }
    for job_id in work.submitted.live_ids() {
        waiting.push(format!("job {job_id}"));
    }
    waiting
}

fn current_wave_complete(work: &SupervisorWork, shutdown: &ShutdownRuntime) -> bool {
    wave_complete(work, shutdown, shutdown.current_wave)
}

fn wave_complete(work: &SupervisorWork, shutdown: &ShutdownRuntime, wave_index: usize) -> bool {
    let Some(wave) = shutdown.plan.stop_waves.get(wave_index) else {
        return shutdown.stop_deadlines.is_empty()
            && shutdown.post_kill_deadlines.is_empty()
            && !live_submitted_jobs_remain(work);
    };
    wave.services
        .iter()
        .all(|participant| participant_done_for_shutdown(work, &participant.service))
}

/// Whether the shutdown has nothing left to do for this participant: it is
/// stopped, or the service table no longer has it. A service whose
/// definition was withdrawn while it ran is discarded from the table once it
/// stops, and a shutdown stops it: first-boot setup's oobe-tui is withdrawn
/// that way. Counting it as not done held its wave open until the global
/// timeout, which could not finish it either (PEI-1216).
pub(in crate::supervisor) fn participant_done_for_shutdown(
    work: &SupervisorWork,
    service: &str,
) -> bool {
    work.services
        .runtime(service)
        .is_none_or(|runtime| service_done_for_shutdown(runtime.state))
}

pub(super) fn service_done_for_shutdown(state: ServiceState) -> bool {
    matches!(
        state,
        ServiceState::Inactive
            | ServiceState::Failed
            | ServiceState::Abandoned
            | ServiceState::Skipped
    )
}
