use crate::boot::BootMode;
use crate::boundary::UndecodableService;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::{ServiceDefinition, validate_service_graph};

use super::graph::{BlockedServiceDraft, build_phase2_boot_graph, safe_mode_downgrade_findings};
use super::model::{
    BlockedReason, BlockedService, Phase2BootPlan, Phase2BootPlanError, PreparedStart,
};

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
        Phase2PlanContext::default(),
    )
}

/// What the planner needs beyond the definitions themselves.
///
/// Grouped rather than passed as two more slices: both describe services the
/// plan must account for but that are not ordinary members of `services` —
/// ones already satisfied by a retained process, and ones whose key would not
/// decode at all.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Phase2PlanContext<'a> {
    pub(crate) retained_satisfied: &'a [String],
    pub(crate) undecodable: &'a [UndecodableService],
}

pub(crate) fn prepare_phase2_boot_plan_with_retained(
    mode: BootMode,
    services: &[ServiceDefinition],
    max_parallel_starts: u32,
    observed_at_ns: u64,
    operation_ids: &mut OperationIdAllocator,
    job_ids: &mut JobIdAllocator,
    context: Phase2PlanContext<'_>,
) -> Result<Phase2BootPlan, Phase2BootPlanError> {
    let Phase2PlanContext {
        retained_satisfied,
        undecodable,
    } = context;
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
            safe_mode_downgrade: Vec::new(),
            warnings: Vec::new(),
        });
    }

    // The findings are kept, not just the verdict: the Safe-mode rebuild
    // discards the Full-mode graph, so this is the only point at which what
    // forced the downgrade still exists.
    let safe_mode_downgrade = if mode == BootMode::Full {
        safe_mode_downgrade_findings(services)?
    } else {
        Vec::new()
    };
    let effective_mode = if safe_mode_downgrade.is_empty() {
        mode
    } else {
        BootMode::Safe
    };

    // Validation findings are not consulted here: Phase 2 blocks the
    // services it cannot start individually, rather than refusing the
    // whole graph the way a reload does, and the blocking below is what
    // produces those. The warnings are the part a boot has no other way
    // to surface, so they are carried out on the plan and emitted with
    // the rest of its audit record.
    let warnings = validate_service_graph(services)
        .map(|validation| validation.warnings)
        .unwrap_or_default();

    let graph = build_phase2_boot_graph(effective_mode, services)?;
    // Keys that exist but will not decode are Failed with ValidationError,
    // exactly as a definition that fails graph validation is. They are not in
    // `services` -- there is no definition to put there -- so they are seeded
    // here rather than found by the graph walk. Anything depending on one
    // fails through the ordinary DependencyFailure propagation, so the blast
    // radius is bounded by what actually needed it.
    let mut blocked = graph.blocked;
    for service in undecodable {
        blocked.push(BlockedServiceDraft {
            service: service.name.clone(),
            reason: BlockedReason::ValidationError {
                message: format!("Service definition failed to decode: {}", service.message),
            },
            additional_reasons: Vec::new(),
        });
    }
    let order = graph
        .ordered_startable
        .into_iter()
        .filter(|service| !retained_satisfied.contains(&service.name))
        .collect::<Vec<_>>();
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
            additional_reasons: blocked.additional_reasons,
        })
        .collect();

    Ok(Phase2BootPlan {
        safe_mode_downgrade,
        mode: effective_mode,
        observed_at_ns,
        max_parallel_starts,
        starts,
        blocked: blocked_services,
        warnings,
    })
}
