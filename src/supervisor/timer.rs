use super::dispatch::{SupervisorTimerAction, SupervisorTimerDispatch};
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

mod classification;
mod pending;
mod start;

use classification::{TimerFiringAction, classify_timer_firing};
pub(super) use pending::start_pending_timer_runs_after_terminal;
use start::dispatch_timer_start_for_work;

impl Supervisor {
    pub fn handle_timer_firing(
        &mut self,
        service: &str,
        schedule: &str,
        observed_at_ns: u64,
    ) -> Result<SupervisorTimerDispatch, SupervisorError> {
        let definition = self.services.definition(service).ok_or_else(|| {
            SupervisorError::MissingStartCredentials {
                service: service.to_string(),
            }
        })?;
        let runtime = self.services.runtime(service).ok_or_else(|| {
            SupervisorError::MissingStartCredentials {
                service: service.to_string(),
            }
        })?;

        let action = if definition.disabled {
            TimerFiringAction::Disabled
        } else {
            classify_timer_firing(definition, runtime.state)
        };

        match action {
            TimerFiringAction::Start => {
                let mut work = SupervisorWork::from_supervisor(self);
                let timer_start = dispatch_timer_start_for_work(
                    &mut work,
                    service,
                    observed_at_ns,
                    self.settings.phase2.max_parallel_starts,
                )?;
                work.commit(self);
                Ok(SupervisorTimerDispatch {
                    service: service.to_string(),
                    schedule: schedule.to_string(),
                    action: SupervisorTimerAction::Start {
                        requested_operation_id: timer_start.requested_operation_id,
                        outcome: Box::new(timer_start.outcome),
                        context_id: timer_start.context_id,
                        start_dispatches: timer_start.start_dispatches,
                    },
                })
            }
            TimerFiringAction::PendingOneshot => {
                let newly_pending = self.services.set_pending_timer(service).map_err(|source| {
                    SupervisorError::Lifecycle(
                        crate::control::lifecycle::LifecycleCommandError::ServiceTable(source),
                    )
                })?;
                Ok(SupervisorTimerDispatch {
                    service: service.to_string(),
                    schedule: schedule.to_string(),
                    action: SupervisorTimerAction::PendingOneshot { newly_pending },
                })
            }
            TimerFiringAction::SimpleNoop => Ok(SupervisorTimerDispatch {
                service: service.to_string(),
                schedule: schedule.to_string(),
                action: SupervisorTimerAction::SimpleNoop,
            }),
            TimerFiringAction::StateNoop { state } => Ok(SupervisorTimerDispatch {
                service: service.to_string(),
                schedule: schedule.to_string(),
                action: SupervisorTimerAction::StateNoop { state },
            }),
            TimerFiringAction::Disabled => Ok(SupervisorTimerDispatch {
                service: service.to_string(),
                schedule: schedule.to_string(),
                action: SupervisorTimerAction::Disabled,
            }),
        }
    }
}
