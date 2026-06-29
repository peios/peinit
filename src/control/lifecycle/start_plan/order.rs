use std::collections::BTreeSet;

use crate::service::{ServiceTable, dependency_start_order, start_order_dependency_targets};

use super::model::OnDemandStartPlanError;

pub(super) fn dependency_order(
    startable: &BTreeSet<String>,
    services: &ServiceTable,
) -> Result<Vec<String>, OnDemandStartPlanError> {
    dependency_start_order(startable, |service| {
        services
            .get(service)
            .map(|entry| start_order_dependency_targets(&entry.definition))
            .unwrap_or_default()
    })
    .map_err(|cycle| OnDemandStartPlanError::Cycle {
        services: cycle.services,
    })
}
