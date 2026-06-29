use crate::boot::BootMode;
use crate::boot::phase2::Phase2BootPlanError;
use crate::service::ServiceDefinition;

use super::closure::collect_boot_closure;
use super::conflict::block_unresolvable_conflicts;
use super::model::Phase2BootGraph;
use super::service::{service_map, startable_service};
use super::validation::start_order_after_validation;

pub(in crate::boot::phase2) fn build_phase2_boot_graph(
    mode: BootMode,
    services: &[ServiceDefinition],
) -> Result<Phase2BootGraph, Phase2BootPlanError> {
    let by_name = service_map(services)?;
    let mut closure = collect_boot_closure(mode, services, &by_name);
    block_unresolvable_conflicts(&closure.included, &by_name, &mut closure.blocked);
    let order = start_order_after_validation(&closure.included, &by_name, &mut closure.blocked);
    let mut ordered_startable = Vec::with_capacity(order.len());
    for service in order {
        ordered_startable.push(startable_service(service, &by_name)?);
    }

    Ok(Phase2BootGraph {
        ordered_startable,
        blocked: closure.blocked.into_values().collect(),
    })
}
