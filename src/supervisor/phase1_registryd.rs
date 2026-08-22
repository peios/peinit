use crate::boot::BootMode;
use crate::boot::phase2::{Phase2BootPlan, Phase2BootPlanError, prepare_phase2_boot_plan};
use crate::operation::store::OperationStoreError;
use crate::service::{ServiceDefinition, ServiceTable, ServiceTableError};

use super::dispatch::SupervisorBootDispatch;
use super::state::{Supervisor, SupervisorError};
use super::work::SupervisorWork;

impl Supervisor {
    pub fn prepare_phase1_registryd_start(
        &mut self,
        observed_at_ns: u64,
    ) -> Result<SupervisorBootDispatch, SupervisorError> {
        let mut work = SupervisorWork::from_supervisor(self);
        let definition = ServiceDefinition::compiled_in_registryd();
        let services = vec![definition];
        let plan = phase1_registryd_plan(&services, observed_at_ns, &mut work)?;
        let service_table =
            ServiceTable::from_boot_snapshot(services).map_err(phase1_service_table_error)?;
        let dispatch = work
            .operations
            .dispatch_phase2_boot_plan(&plan)
            .map_err(phase1_operation_error)?;
        work.services = service_table;
        let definitions = definitions_from_table(&work.services);
        let context_id = work
            .graph
            .create_boot_context(&plan, &definitions)
            .map_err(SupervisorError::GraphContext)?;
        let start_dispatches = work.release_for_context(context_id, 1, observed_at_ns)?;

        work.commit(self);

        Ok(SupervisorBootDispatch {
            plan,
            // Phase 1 runs before the Phase 2 config read, so there is nothing
            // to report here.
            config_warnings: Vec::new(),
            operation_dispatch: dispatch,
            context_id,
            start_dispatches,
        })
    }
}

fn phase1_registryd_plan(
    services: &[ServiceDefinition],
    observed_at_ns: u64,
    work: &mut SupervisorWork,
) -> Result<Phase2BootPlan, SupervisorError> {
    prepare_phase2_boot_plan(
        BootMode::Full,
        services,
        1,
        observed_at_ns,
        &mut work.operation_ids,
        &mut work.job_ids,
    )
    .map_err(phase1_plan_error)
}

fn definitions_from_table(services: &ServiceTable) -> Vec<ServiceDefinition> {
    services
        .service_names()
        .into_iter()
        .filter_map(|service| services.definition(service).cloned())
        .collect()
}

fn phase1_plan_error(error: Phase2BootPlanError) -> SupervisorError {
    SupervisorError::Phase2Boot(crate::boot::phase2::Phase2BootRunError::Plan(error))
}

fn phase1_operation_error(error: OperationStoreError) -> SupervisorError {
    SupervisorError::Phase2Boot(crate::boot::phase2::Phase2BootRunError::Dispatch(error))
}

fn phase1_service_table_error(error: ServiceTableError) -> SupervisorError {
    SupervisorError::Phase2Boot(crate::boot::phase2::Phase2BootRunError::ServiceTable(error))
}
