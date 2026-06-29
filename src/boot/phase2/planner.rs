use crate::boot::BootMode;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::ServiceDefinition;

use super::graph::{build_phase2_boot_graph, safe_mode_required_for_full_boot};
use super::model::{BlockedService, Phase2BootPlan, Phase2BootPlanError, PreparedStart};

pub fn prepare_phase2_boot_plan(
    mode: BootMode,
    services: &[ServiceDefinition],
    max_parallel_starts: u32,
    observed_at_ns: u64,
    operation_ids: &mut OperationIdAllocator,
    job_ids: &mut JobIdAllocator,
) -> Result<Phase2BootPlan, Phase2BootPlanError> {
    prepare_phase2_boot_plan_with_retained(
        mode,
        services,
        max_parallel_starts,
        observed_at_ns,
        operation_ids,
        job_ids,
        &[],
    )
}

pub(crate) fn prepare_phase2_boot_plan_with_retained(
    mode: BootMode,
    services: &[ServiceDefinition],
    max_parallel_starts: u32,
    observed_at_ns: u64,
    operation_ids: &mut OperationIdAllocator,
    job_ids: &mut JobIdAllocator,
    retained_satisfied: &[String],
) -> Result<Phase2BootPlan, Phase2BootPlanError> {
    if max_parallel_starts == 0 {
        return Err(Phase2BootPlanError::InvalidMaxParallelStarts);
    }
    if mode == BootMode::Recovery {
        return Ok(Phase2BootPlan {
            mode,
            observed_at_ns,
            max_parallel_starts,
            starts: Vec::new(),
            blocked: Vec::new(),
        });
    }

    let effective_mode = if mode == BootMode::Full && safe_mode_required_for_full_boot(services)? {
        BootMode::Safe
    } else {
        mode
    };

    let graph = build_phase2_boot_graph(effective_mode, services)?;
    let order = graph
        .ordered_startable
        .into_iter()
        .filter(|service| !retained_satisfied.contains(&service.name))
        .collect::<Vec<_>>();
    let blocked = graph.blocked;
    let start_count = order.len();
    let mut next_operation_ids = operation_ids.clone();
    let mut next_job_ids = job_ids.clone();
    let allocated_job_ids = next_job_ids
        .allocate_batch(order.len(), observed_at_ns)
        .map_err(Phase2BootPlanError::JobIdAllocation)?;
    let allocated_operation_ids = next_operation_ids
        .allocate_batch(order.len() + blocked.len(), observed_at_ns)
        .map_err(Phase2BootPlanError::OperationIdAllocation)?;
    *operation_ids = next_operation_ids;
    *job_ids = next_job_ids;

    let starts = order
        .into_iter()
        .zip(allocated_operation_ids.iter().copied())
        .zip(allocated_job_ids)
        .map(|((service, operation_id), job_id)| PreparedStart {
            cause: service.cause,
            service: service.name,
            operation_id,
            job_id,
            identity: service.identity,
        })
        .collect();
    let blocked_services = blocked
        .into_iter()
        .zip(allocated_operation_ids.into_iter().skip(start_count))
        .map(|(blocked, operation_id)| BlockedService {
            service: blocked.service,
            operation_id,
            reason: blocked.reason,
        })
        .collect();

    Ok(Phase2BootPlan {
        mode: effective_mode,
        observed_at_ns,
        max_parallel_starts,
        starts,
        blocked: blocked_services,
    })
}
