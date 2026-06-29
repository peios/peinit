use crate::boot::phase2::{Phase2BootRun, Phase2BootRunError, run_phase2_boot_with_retained};
use crate::boundary::{Clock, RegistryClient};
use crate::service::runtime::{ServiceState, ServiceTransition};
use crate::service::{ServiceDefinition, ServiceTable, ServiceTableError};

use super::dispatch::SupervisorBootDispatch;
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    pub fn run_phase2_boot<R, C>(
        &mut self,
        registry: &mut R,
        clock: &mut C,
    ) -> Result<SupervisorBootDispatch, SupervisorError>
    where
        R: RegistryClient + ?Sized,
        C: Clock + ?Sized,
    {
        let mut work = SupervisorWork::from_supervisor(self);
        let retained_satisfied = retained_satisfied_services(&work.services);
        let Phase2BootRun {
            settings: phase2_settings,
            services_schema_version: _,
            config_warnings: _,
            shutdown_settings,
            control_security,
            control_limits,
            log_config,
            service_table,
            global_environment,
            eventd_log_socket_path,
            plan,
            dispatch,
        } = run_phase2_boot_with_retained(
            self.settings.phase2,
            registry,
            clock,
            &mut work.operation_ids,
            &mut work.job_ids,
            &mut work.operations,
            &retained_satisfied,
        )
        .map_err(SupervisorError::Phase2Boot)?;

        let definitions = definitions_from_table(&service_table);
        merge_phase2_service_table(&mut work.services, service_table, &retained_satisfied)
            .map_err(|error| {
                SupervisorError::Phase2Boot(Phase2BootRunError::ServiceTable(error))
            })?;
        apply_blocked_phase2_services(&mut work.services, &plan).map_err(|error| {
            SupervisorError::Phase2Boot(Phase2BootRunError::ServiceTable(error))
        })?;
        work.boot_success.configure_phase2(
            &work.services,
            &plan,
            &retained_satisfied,
            phase2_settings.boot_success_grace_secs,
        );
        work.global_environment = global_environment;
        work.eventd_log_socket_path = eventd_log_socket_path;
        work.control_security = control_security;
        work.control_limits = control_limits;
        work.log_config = log_config;
        let context_id = work
            .graph
            .create_boot_context(&plan, &definitions)
            .map_err(SupervisorError::GraphContext)?;

        let start_dispatches = work.release_for_context(
            context_id,
            phase2_settings.max_parallel_starts,
            plan.observed_at_ns,
        )?;

        work.commit(self);
        self.settings.phase2 = phase2_settings;
        self.settings.shutdown = shutdown_settings;

        Ok(SupervisorBootDispatch {
            plan,
            operation_dispatch: dispatch,
            context_id,
            start_dispatches,
        })
    }
}

fn apply_blocked_phase2_services(
    services: &mut ServiceTable,
    plan: &crate::boot::phase2::Phase2BootPlan,
) -> Result<(), ServiceTableError> {
    for blocked in &plan.blocked {
        let Some(runtime) = services.runtime(&blocked.service) else {
            return Err(ServiceTableError::UnknownService {
                service: blocked.service.clone(),
            });
        };
        if runtime.state != ServiceState::Inactive {
            continue;
        }
        services.transition_service(
            &blocked.service,
            ServiceTransition {
                to: ServiceState::Failed,
                cause: blocked.reason.transition_cause(),
            },
        )?;
    }
    Ok(())
}

fn merge_phase2_service_table(
    retained: &mut ServiceTable,
    phase2: ServiceTable,
    retained_satisfied: &[String],
) -> Result<(), ServiceTableError> {
    if retained.service_names().is_empty() {
        *retained = phase2;
        return Ok(());
    }
    let mut definitions = definitions_from_table(&phase2);
    for service in retained_satisfied {
        if definitions
            .iter()
            .any(|definition| &definition.name == service)
        {
            continue;
        }
        // A retained Phase-1 service the Phase-2 registry does not define — the
        // compiled-in registryd, which bootstraps the registry and so can never
        // be a registry entry. Carry its existing (compiled-in) definition into
        // the snapshot so apply_definition_snapshot keeps the running service
        // instead of treating it as removed (or, previously, erroring).
        match retained.definition(service) {
            Some(definition) => definitions.push(definition.clone()),
            None => {
                return Err(ServiceTableError::UnknownService {
                    service: service.clone(),
                });
            }
        }
    }
    retained.apply_definition_snapshot(definitions)?;
    Ok(())
}

fn definitions_from_table(services: &ServiceTable) -> Vec<ServiceDefinition> {
    services
        .service_names()
        .into_iter()
        .filter_map(|service| services.definition(service).cloned())
        .collect()
}

fn retained_satisfied_services(services: &ServiceTable) -> Vec<String> {
    services
        .service_names()
        .into_iter()
        .filter(|service| {
            services
                .runtime(service)
                .is_some_and(|runtime| runtime.state.satisfies_dependents())
        })
        .map(ToString::to_string)
        .collect()
}
