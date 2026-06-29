use crate::boundary::{Clock, ProcessController};
use crate::control::lifecycle::{LifecycleCommand, LifecycleCommandError, LifecycleCommandRequest};
use crate::job::{ServiceCgroupKind, service_cgroup_root_path, service_job_cgroup_path};
use crate::security::TokenSummary;
use crate::service::runtime::ServiceState;

use super::super::cgroup_cleanup::cleanup_service_cgroup_tree;
use super::super::dispatch::SupervisorLifecycleDispatch;
use super::super::state::{Supervisor, SupervisorError};
use super::super::work::SupervisorWork;
use super::finish::finish_lifecycle_outcome;
use super::request::{admit_supervisor_lifecycle_command, allocate_request_id};

impl Supervisor {
    pub fn run_lifecycle_command_with_process_controller<C, P>(
        &mut self,
        command: LifecycleCommand,
        service: impl Into<String>,
        caller: Option<TokenSummary>,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
    {
        let service = service.into();
        if command == LifecycleCommand::Reset && service_is_abandoned(&self.services, &service)? {
            return self.run_abandoned_reset_command(service, caller, controller, clock);
        }
        self.run_lifecycle_command(command, service, caller, clock)
    }

    fn run_abandoned_reset_command<C, P>(
        &mut self,
        service: String,
        caller: Option<TokenSummary>,
        controller: &mut P,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
    {
        if let Some(shutdown) = &self.shutdown {
            return Err(SupervisorError::Shutdown(
                crate::shutdown::ShutdownError::AlreadyInProgress {
                    kind: shutdown.kind,
                },
            ));
        }
        let created_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
        let generation = abandoned_generation(&self.services, &service)?;
        let main_cgroup_id = service_job_cgroup_path(&service, generation, ServiceCgroupKind::Main);
        let main_populated = controller
            .cgroup_populated(&main_cgroup_id)
            .map_err(SupervisorError::ProcessControl)?;
        if !main_populated {
            let root_cgroup_id = service_cgroup_root_path(&service, generation);
            cleanup_service_cgroup_tree(controller, &root_cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
        }

        let mut work = SupervisorWork::from_supervisor(self);
        let request_id = allocate_request_id(&mut work, created_at_ns)?;
        let outcome = admit_supervisor_lifecycle_command(
            &mut work,
            LifecycleCommandRequest {
                id: request_id,
                command: LifecycleCommand::Reset,
                service: service.clone(),
                caller,
                created_at_ns,
            },
        )?;
        let mut dispatch = finish_lifecycle_outcome(
            self,
            work,
            outcome,
            self.settings.phase2.max_parallel_starts,
            created_at_ns,
        )?;
        if main_populated {
            dispatch
                .lifecycle_warnings
                .push(abandoned_reset_warning(&service));
        }
        Ok(dispatch)
    }
}

pub(in crate::supervisor::lifecycle) fn service_is_abandoned(
    services: &crate::service::ServiceTable,
    service: &str,
) -> Result<bool, SupervisorError> {
    let entry = services.get(service).ok_or_else(|| {
        SupervisorError::Lifecycle(LifecycleCommandError::UnknownService {
            service: service.to_string(),
        })
    })?;
    if entry.definition_removed {
        return Err(SupervisorError::Lifecycle(
            LifecycleCommandError::DefinitionRemoved {
                service: service.to_string(),
            },
        ));
    }
    Ok(entry.runtime.state == ServiceState::Abandoned)
}

fn abandoned_generation(
    services: &crate::service::ServiceTable,
    service: &str,
) -> Result<u64, SupervisorError> {
    let entry = services.get(service).ok_or_else(|| {
        SupervisorError::Lifecycle(LifecycleCommandError::UnknownService {
            service: service.to_string(),
        })
    })?;
    if entry.runtime.state != ServiceState::Abandoned {
        return Err(SupervisorError::Lifecycle(
            LifecycleCommandError::InvalidState {
                service: service.to_string(),
                command: LifecycleCommand::Reset,
                state: entry.runtime.state,
            },
        ));
    }
    Ok(entry.runtime.cgroup_generation)
}

fn abandoned_reset_warning(service: &str) -> String {
    format!(
        "abandoned main cgroup for service {service} is still populated after reset -- cgroup remains leaked; underlying D-state process requires investigation"
    )
}
