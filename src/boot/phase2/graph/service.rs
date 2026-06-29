use std::collections::BTreeMap;

use crate::boot::phase2::{Phase2BootPlanError, StartCause};
use crate::service::ServiceDefinition;

use super::model::{ServiceMap, StartableService};

pub(in crate::boot::phase2::graph) fn service_map(
    services: &[ServiceDefinition],
) -> Result<ServiceMap<'_>, Phase2BootPlanError> {
    let mut by_name = BTreeMap::new();
    for service in services {
        if by_name.insert(service.name.as_str(), service).is_some() {
            return Err(Phase2BootPlanError::DuplicateService {
                service: service.name.clone(),
            });
        }
    }
    Ok(by_name)
}

pub(in crate::boot::phase2::graph) fn startable_service(
    service: String,
    by_name: &ServiceMap<'_>,
) -> Result<StartableService, Phase2BootPlanError> {
    let definition = by_name.get(service.as_str()).ok_or_else(|| {
        Phase2BootPlanError::MissingServiceDefinition {
            service: service.clone(),
        }
    })?;
    Ok(StartableService {
        name: service,
        cause: if definition.has_boot_trigger() {
            StartCause::ExplicitStart
        } else {
            StartCause::DependencyStart
        },
        identity: definition.identity.clone(),
    })
}
