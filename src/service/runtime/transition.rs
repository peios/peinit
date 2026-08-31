use super::model::{ServiceRuntimeSnapshot, ServiceState, TransitionCause};
use super::rules::is_allowed_transition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTransition {
    pub to: ServiceState,
    pub cause: TransitionCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTransitionEvent {
    pub service: String,
    pub from: ServiceState,
    pub to: ServiceState,
    pub cause: TransitionCause,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceTransitionError {
    InvalidTransition {
        service: String,
        from: ServiceState,
        to: ServiceState,
        cause: TransitionCause,
    },
}

impl ServiceRuntimeSnapshot {
    pub fn transition(
        &mut self,
        transition: ServiceTransition,
    ) -> Result<ServiceTransitionEvent, ServiceTransitionError> {
        if !is_allowed_transition(self.state, transition.to, transition.cause) {
            return Err(ServiceTransitionError::InvalidTransition {
                service: self.service.clone(),
                from: self.state,
                to: transition.to,
                cause: transition.cause,
            });
        }

        let from = self.state;
        if transition.to == ServiceState::Starting {
            self.generation += 1;
            self.status_text = None;
            self.stopping_acknowledged = false;
            self.pending_timer = false;
            self.health = super::model::ServiceHealthSnapshot::unknown();
        }
        if transition.to == ServiceState::Stopping || from == ServiceState::Stopping {
            self.stopping_timeout = None;
        }
        self.state = transition.to;
        self.cause = Some(transition.cause);
        if !self.state.satisfies_dependents() {
            self.dependent_satisfied_since_ns = None;
            // A level is a claim about a process that is currently making
            // it. Keeping one across a stop would hold a dependent open on
            // a promise nobody is keeping — the exact failure the whole
            // level mechanism exists to avoid.
            self.level = None;
        }

        Ok(ServiceTransitionEvent {
            service: self.service.clone(),
            from,
            to: transition.to,
            cause: transition.cause,
            generation: self.generation,
        })
    }
}
