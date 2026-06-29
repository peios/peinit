use crate::execution::notify::{NotifyAppliedField, NotifyApplyDispatch};
use crate::service::runtime::ServiceState;
use crate::supervisor::dispatch::{
    SupervisorWatchdogNotifyDispatch, SupervisorWatchdogNotifyOutcome,
};
use crate::supervisor::work::SupervisorWork;

use super::WatchdogError;

pub(in crate::supervisor) fn apply_watchdog_notify_fields(
    work: &mut SupervisorWork,
    notify: &NotifyApplyDispatch,
    observed_at_ns: u64,
) -> Result<Vec<SupervisorWatchdogNotifyDispatch>, WatchdogError> {
    let mut dispatches = Vec::new();
    for field in &notify.applied_fields {
        match field {
            NotifyAppliedField::Watchdog => {
                dispatches.push(apply_watchdog_keepalive(work, notify, observed_at_ns));
            }
            NotifyAppliedField::WatchdogUsec { value } => {
                dispatches.push(apply_watchdog_interval_update(
                    work,
                    notify,
                    value,
                    observed_at_ns,
                )?);
            }
            _ => {}
        }
    }
    Ok(dispatches)
}

fn apply_watchdog_keepalive(
    work: &mut SupervisorWork,
    notify: &NotifyApplyDispatch,
    observed_at_ns: u64,
) -> SupervisorWatchdogNotifyDispatch {
    let update = if active_current_generation(work, notify) {
        work.watchdog.reset(
            &notify.sender.service,
            notify.sender.generation,
            observed_at_ns,
        )
    } else {
        crate::supervisor::watchdog::WatchdogUpdate::Ignored
    };
    SupervisorWatchdogNotifyDispatch {
        service: notify.sender.service.clone(),
        generation: notify.sender.generation,
        outcome: update.into(),
    }
}

fn apply_watchdog_interval_update(
    work: &mut SupervisorWork,
    notify: &NotifyApplyDispatch,
    value: &str,
    observed_at_ns: u64,
) -> Result<SupervisorWatchdogNotifyDispatch, WatchdogError> {
    let interval_usec = value
        .parse::<u64>()
        .map_err(|_| WatchdogError::InvalidRuntimeUpdate {
            service: notify.sender.service.clone(),
            value: value.to_string(),
        })?;
    let update = if let Some(cgroup_generation) = active_current_cgroup_generation(work, notify) {
        work.watchdog.update_interval(
            &notify.sender.service,
            notify.sender.generation,
            cgroup_generation,
            interval_usec,
            observed_at_ns,
        )
    } else {
        crate::supervisor::watchdog::WatchdogUpdate::Ignored
    };
    Ok(SupervisorWatchdogNotifyDispatch {
        service: notify.sender.service.clone(),
        generation: notify.sender.generation,
        outcome: update.into(),
    })
}

fn active_current_generation(work: &SupervisorWork, notify: &NotifyApplyDispatch) -> bool {
    work.services
        .runtime(&notify.sender.service)
        .is_some_and(|runtime| {
            runtime.state == ServiceState::Active && runtime.generation == notify.sender.generation
        })
}

fn active_current_cgroup_generation(
    work: &SupervisorWork,
    notify: &NotifyApplyDispatch,
) -> Option<u64> {
    work.services
        .runtime(&notify.sender.service)
        .filter(|runtime| {
            runtime.state == ServiceState::Active && runtime.generation == notify.sender.generation
        })
        .map(|runtime| runtime.cgroup_generation)
}

impl From<crate::supervisor::watchdog::WatchdogUpdate> for SupervisorWatchdogNotifyOutcome {
    fn from(update: crate::supervisor::watchdog::WatchdogUpdate) -> Self {
        match update {
            crate::supervisor::watchdog::WatchdogUpdate::Armed { due_at_ns } => {
                Self::Armed { due_at_ns }
            }
            crate::supervisor::watchdog::WatchdogUpdate::Disabled => Self::Disabled,
            crate::supervisor::watchdog::WatchdogUpdate::Ignored => Self::Ignored,
        }
    }
}
