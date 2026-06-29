use crate::boot::phase2::Phase2BootPlanError;
use crate::service::{dependency_start_order, start_order_dependency_targets};

use super::model::{ServiceMap, StartableSet};

pub(super) fn dependency_order(
    startable: &StartableSet,
    by_name: &ServiceMap<'_>,
) -> Result<Vec<String>, Phase2BootPlanError> {
    dependency_start_order(startable, |service| {
        by_name
            .get(service)
            .map(|definition| start_order_dependency_targets(definition))
            .unwrap_or_default()
    })
    .map_err(|cycle| Phase2BootPlanError::Cycle {
        services: cycle.services,
    })
}
