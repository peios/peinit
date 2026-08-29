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
            if shutdown.stop_deadlines.is_empty()
                && shutdown.post_kill_deadlines.is_empty()
                && !live_submitted_jobs_remain(work)
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

fn current_wave_complete(work: &SupervisorWork, shutdown: &ShutdownRuntime) -> bool {
    wave_complete(work, shutdown, shutdown.current_wave)
}

fn wave_complete(work: &SupervisorWork, shutdown: &ShutdownRuntime, wave_index: usize) -> bool {
    let Some(wave) = shutdown.plan.stop_waves.get(wave_index) else {
        return shutdown.stop_deadlines.is_empty()
            && shutdown.post_kill_deadlines.is_empty()
            && !live_submitted_jobs_remain(work);
    };
    wave.services.iter().all(|participant| {
        work.services
            .runtime(&participant.service)
            .is_some_and(|runtime| service_done_for_shutdown(runtime.state))
    })
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
