use crate::boundary::{
    Clock, FilesystemCheckHelperLauncher, ProcessController, ProcessLauncher, TokenProvider,
};
use crate::supervisor::{
    Supervisor, SupervisorControlLaunchResult, SupervisorError, SupervisorHealthCheckLaunchResult,
    SupervisorPostStartHookLaunchResult, SupervisorServiceLaunchDispatch,
    SupervisorStartHookLaunchResult, SupervisorSubmittedLaunchResult,
};

use super::model::{
    RuntimeWorkPumpContext, RuntimeWorkPumpError, RuntimeWorkPumpStep, RuntimeWorkPumpTurn,
};

pub fn drain_runtime_work_queues<C, P, T, L, F>(
    supervisor: &mut Supervisor,
    context: &mut RuntimeWorkPumpContext<'_, C, P, T, L, F>,
) -> Result<RuntimeWorkPumpTurn, RuntimeWorkPumpError>
where
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    T: TokenProvider + ?Sized,
    L: ProcessLauncher + ?Sized,
    F: FilesystemCheckHelperLauncher + ?Sized,
{
    let mut turn = RuntimeWorkPumpTurn::default();

    for _ in 0..context.config.max_iterations {
        let step = pump_one_runtime_work_iteration(supervisor, context)?;
        if !step.progressed() {
            return Ok(turn);
        }
        turn.extend(step);
    }

    if !has_pending_runtime_work(supervisor) {
        return Ok(turn);
    }

    Err(RuntimeWorkPumpError::IterationLimitExceeded {
        limit: context.config.max_iterations,
        pending_control_operations: supervisor.pending_control_operations().len(),
        pending_filesystem_check_launches: supervisor.pending_pre_start_check_launches().len(),
        pending_start_hook_launches: supervisor.pending_start_hook_launch_jobs().len(),
        pending_post_hook_launches: supervisor.pending_post_hook_launch_jobs().len(),
        pending_control_launches: supervisor.pending_control_launch_jobs().len(),
        pending_health_check_launches: supervisor.pending_health_check_launch_jobs().len(),
        pending_service_launches: supervisor.pending_launch_jobs().len(),
        pending_submitted_launches: supervisor.pending_submitted_launch_jobs().len(),
    })
}

fn pump_one_runtime_work_iteration<C, P, T, L, F>(
    supervisor: &mut Supervisor,
    context: &mut RuntimeWorkPumpContext<'_, C, P, T, L, F>,
) -> Result<RuntimeWorkPumpStep, RuntimeWorkPumpError>
where
    C: Clock + ?Sized,
    P: ProcessController + ?Sized,
    T: TokenProvider + ?Sized,
    L: ProcessLauncher + ?Sized,
    F: FilesystemCheckHelperLauncher + ?Sized,
{
    // An operation queued behind one that has since finished is admitted now,
    // against the service's current state, before the boundary drains
    // (PEI-824).
    let promoted_operations = if supervisor.has_ready_queued_operations() {
        let now_ns = context
            .clock
            .monotonic_ns()
            .map_err(|error| RuntimeWorkPumpError::Supervisor(SupervisorError::Clock(error)))?;
        supervisor
            .promote_queued_operations(now_ns)
            .map_err(RuntimeWorkPumpError::Supervisor)?
    } else {
        Vec::new()
    };

    let pending_control_before = supervisor.pending_control_operations().len();
    let control_operation = if pending_control_before > 0 {
        supervisor
            .execute_next_pending_control_operation(context.controller, context.clock)
            .map_err(RuntimeWorkPumpError::Supervisor)?
    } else {
        None
    };
    let pending_control_after = supervisor.pending_control_operations().len();
    let control_operation_failures = supervisor.take_control_operation_failures();
    // A hold on a service in Backoff that its state has decided but no
    // transition funnel has settled — a definition withdrawn on reload has
    // no other hook — is settled here, with a clock (PEI-821).
    if supervisor.has_settleable_held_restarts() {
        let now_ns = context
            .clock
            .monotonic_ns()
            .map_err(|error| RuntimeWorkPumpError::Supervisor(SupervisorError::Clock(error)))?;
        supervisor
            .settle_held_restarts_now(now_ns)
            .map_err(RuntimeWorkPumpError::Supervisor)?;
    }
    let held_restart_settlements = supervisor.take_held_restart_settlements();
    let stale_control_operations = if control_operation.is_none() {
        pending_control_before
            .saturating_sub(pending_control_after)
            .saturating_sub(control_operation_failures.len())
    } else {
        0
    };

    let filesystem_check_launch = if supervisor.pending_pre_start_check_launches().is_empty() {
        None
    } else {
        let launched_at_ns = context
            .clock
            .monotonic_ns()
            .map_err(|error| RuntimeWorkPumpError::Supervisor(SupervisorError::Clock(error)))?;
        supervisor
            .launch_next_pending_filesystem_check_helper(
                context.filesystem_check_launcher,
                launched_at_ns,
            )
            .map_err(RuntimeWorkPumpError::Supervisor)?
    };

    let start_hook_launch = supervisor
        .launch_next_pending_start_hook_job_with_controller(
            context.token_provider,
            context.process_launcher,
            context.clock,
            context.controller,
        )
        .map_err(RuntimeWorkPumpError::Supervisor)?;
    let (start_hook_launch, start_hook_launch_failure, start_hook_pending_setup) =
        match start_hook_launch {
            Some(SupervisorStartHookLaunchResult::Launched(dispatch)) => {
                (Some(dispatch), None, None)
            }
            Some(SupervisorStartHookLaunchResult::Failed(dispatch)) => (None, Some(dispatch), None),
            Some(SupervisorStartHookLaunchResult::PendingSetup(dispatch)) => {
                (None, None, Some(dispatch))
            }
            None => (None, None, None),
        };
    let post_hook_launch = supervisor
        .launch_next_pending_post_hook_job_with_controller(
            context.token_provider,
            context.process_launcher,
            context.clock,
            context.controller,
        )
        .map_err(RuntimeWorkPumpError::Supervisor)?;
    let (post_hook_launch, post_hook_launch_failure, post_hook_pending_setup) =
        match post_hook_launch {
            Some(SupervisorPostStartHookLaunchResult::Launched(dispatch)) => {
                (Some(*dispatch), None, None)
            }
            Some(SupervisorPostStartHookLaunchResult::Failed(dispatch)) => {
                (None, Some(*dispatch), None)
            }
            Some(SupervisorPostStartHookLaunchResult::PendingSetup(dispatch)) => {
                (None, None, Some(dispatch))
            }
            None => (None, None, None),
        };
    let control_launch = supervisor
        .launch_next_pending_control_job(
            context.token_provider,
            context.process_launcher,
            context.clock,
        )
        .map_err(RuntimeWorkPumpError::Supervisor)?;
    let (control_launch, control_pending_setup) = match control_launch {
        Some(SupervisorControlLaunchResult::Launched(dispatch)) => (Some(*dispatch), None),
        Some(SupervisorControlLaunchResult::PendingSetup(dispatch)) => (None, Some(dispatch)),
        None => (None, None),
    };
    let health_check_launch = supervisor
        .launch_next_pending_health_check_job(
            context.token_provider,
            context.process_launcher,
            context.clock,
        )
        .map_err(RuntimeWorkPumpError::Supervisor)?;
    let (
        health_check_launch,
        health_check_launch_failure,
        health_check_launch_cancellation,
        health_check_pending_setup,
    ) = match health_check_launch {
        Some(SupervisorHealthCheckLaunchResult::Launched(dispatch)) => {
            (Some(dispatch), None, None, None)
        }
        Some(SupervisorHealthCheckLaunchResult::Failed(dispatch)) => {
            (None, Some(*dispatch), None, None)
        }
        Some(SupervisorHealthCheckLaunchResult::Cancelled(dispatch)) => {
            (None, None, Some(dispatch), None)
        }
        Some(SupervisorHealthCheckLaunchResult::PendingSetup(dispatch)) => {
            (None, None, None, Some(dispatch))
        }
        None => (None, None, None, None),
    };
    let service_launch = supervisor
        .launch_next_pending_service_job(
            context.token_provider,
            context.process_launcher,
            context.clock,
        )
        .map_err(RuntimeWorkPumpError::Supervisor)?;
    let (service_launch, service_launch_failure, service_pending_setup) = match service_launch {
        Some(SupervisorServiceLaunchDispatch::Launched(dispatch)) => (Some(*dispatch), None, None),
        Some(SupervisorServiceLaunchDispatch::Failed(dispatch)) => (None, Some(*dispatch), None),
        Some(SupervisorServiceLaunchDispatch::PendingSetup(dispatch)) => {
            (None, None, Some(dispatch))
        }
        None => (None, None, None),
    };
    let submitted_launch = supervisor
        .launch_next_pending_submitted_job(
            context.token_provider,
            context.process_launcher,
            context.clock,
            context.controller,
        )
        .map_err(RuntimeWorkPumpError::Supervisor)?;
    let (submitted_launch, submitted_launch_failure, submitted_pending_setup) =
        match submitted_launch {
            Some(SupervisorSubmittedLaunchResult::Launched(dispatch)) => {
                (Some(dispatch), None, None)
            }
            Some(SupervisorSubmittedLaunchResult::Failed(dispatch)) => (None, Some(dispatch), None),
            Some(SupervisorSubmittedLaunchResult::PendingSetup(dispatch)) => {
                (None, None, Some(dispatch))
            }
            None => (None, None, None),
        };
    let pending_process_setups = [
        start_hook_pending_setup,
        post_hook_pending_setup,
        control_pending_setup,
        health_check_pending_setup,
        service_pending_setup,
        submitted_pending_setup,
    ]
    .into_iter()
    .flatten()
    .collect();

    Ok(RuntimeWorkPumpStep {
        promoted_operations,
        control_operation,
        control_operation_failures,
        held_restart_settlements,
        filesystem_check_launch,
        start_hook_launch,
        start_hook_launch_failure,
        post_hook_launch,
        post_hook_launch_failure,
        control_launch,
        pending_process_setups,
        health_check_launch,
        health_check_launch_failure,
        health_check_launch_cancellation,
        service_launch,
        service_launch_failure,
        submitted_launch,
        submitted_launch_failure,
        stale_control_operations,
        stale_launch_entries: supervisor.take_stale_launch_entries(),
    })
}

fn has_pending_runtime_work(supervisor: &Supervisor) -> bool {
    supervisor.has_ready_queued_operations()
        || !supervisor.pending_control_operations().is_empty()
        || !supervisor.pending_pre_start_check_launches().is_empty()
        || !supervisor.pending_start_hook_launch_jobs().is_empty()
        || !supervisor.pending_post_hook_launch_jobs().is_empty()
        || !supervisor.pending_control_launch_jobs().is_empty()
        || !supervisor.pending_health_check_launch_jobs().is_empty()
        || !supervisor.pending_launch_jobs().is_empty()
        || !supervisor.pending_submitted_launch_jobs().is_empty()
}
