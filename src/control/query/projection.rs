use crate::ids::OperationId;
use crate::job::{JobRecord, JobStore};
use crate::operation::store::OperationStore;
use crate::operation::{OperationRecord, OperationState};
use crate::service::runtime::LeakedCgroupKind;
use crate::service::{ServiceEntry, ServiceTable};

use super::model::{
    CurrentJobView, CurrentOperationView, OperationStatusView, QueryError, ServiceListItem,
    ServiceStatusView, ServiceStatusWarning, ServiceStatusWarningType,
};

pub fn service_status(
    services: &ServiceTable,
    operations: &OperationStore,
    jobs: &JobStore,
    service: &str,
) -> Result<ServiceStatusView, QueryError> {
    let entry = services
        .get(service)
        .ok_or_else(|| QueryError::UnknownService {
            service: service.to_string(),
        })?;
    status_from_entry(service, entry, operations, jobs)
}

pub fn list_services(services: &ServiceTable) -> Vec<ServiceListItem> {
    services
        .service_names()
        .into_iter()
        .filter_map(|service| {
            services.get(service).map(|entry| ServiceListItem {
                service: service.to_string(),
                display_name: entry.definition.display_name.clone(),
                description: entry.definition.description.clone(),
                state: entry.runtime.state,
                cause: entry.runtime.cause,
                health: entry
                    .definition
                    .health_check
                    .as_ref()
                    .map(|_| entry.runtime.health.status),
                definition_removed: entry.definition_removed,
            })
        })
        .collect()
}

pub fn operation_status(
    operations: &OperationStore,
    operation_id: OperationId,
) -> Result<OperationStatusView, QueryError> {
    let record = operations
        .get(operation_id)
        .ok_or(QueryError::UnknownOperation { operation_id })?;
    Ok(operation_status_from_record(record))
}

fn status_from_entry(
    service: &str,
    entry: &ServiceEntry,
    operations: &OperationStore,
    jobs: &JobStore,
) -> Result<ServiceStatusView, QueryError> {
    let warnings = service_status_warnings(entry);
    let lifecycle_warnings = lifecycle_warning_messages(&warnings);
    Ok(ServiceStatusView {
        service: service.to_string(),
        display_name: entry.definition.display_name.clone(),
        description: entry.definition.description.clone(),
        state: entry.runtime.state,
        cause: entry.runtime.cause,
        generation: entry.runtime.generation,
        status_text: entry.runtime.status_text.clone(),
        health: entry
            .definition
            .health_check
            .as_ref()
            .map(|_| entry.runtime.health.status),
        definition_removed: entry.definition_removed,
        current_job: current_job(service, jobs)?,
        current_operation: operations
            .current_for_service(service)
            .map(current_operation_view),
        warnings,
        lifecycle_warnings,
    })
}

fn service_status_warnings(entry: &ServiceEntry) -> Vec<ServiceStatusWarning> {
    entry
        .runtime
        .leaked_cgroups
        .iter()
        .map(|leak| ServiceStatusWarning {
            path: leak.path.clone(),
            warning_type: match leak.kind {
                LeakedCgroupKind::ServiceTree => ServiceStatusWarningType::ServiceTree,
                LeakedCgroupKind::Health => ServiceStatusWarningType::Health,
                LeakedCgroupKind::Hooks => ServiceStatusWarningType::Hooks,
                LeakedCgroupKind::Helper => ServiceStatusWarningType::Helper,
            },
            detected_at_ns: leak.detected_at_ns,
        })
        .collect()
}

fn lifecycle_warning_messages(warnings: &[ServiceStatusWarning]) -> Vec<String> {
    if warnings.is_empty() {
        Vec::new()
    } else {
        vec![
            "service has leaked sub-cgroups from a previous generation -- indicates underlying I/O problem requiring investigation"
                .to_string(),
        ]
    }
}

fn current_job(service: &str, jobs: &JobStore) -> Result<Option<CurrentJobView>, QueryError> {
    let Some(job_id) = jobs.current_service_main_job(service) else {
        return Ok(None);
    };
    let record = jobs
        .get(job_id)
        .ok_or_else(|| QueryError::MissingCurrentJobRecord {
            service: service.to_string(),
            job_id,
        })?;
    Ok(Some(current_job_view(record)))
}

fn current_job_view(record: &JobRecord) -> CurrentJobView {
    CurrentJobView {
        id: record.id,
        job_type: record.job_type,
        pid: record.pid,
        started_at_ns: record.started_at_ns,
        identity: record.resolved_identity.clone(),
    }
}

fn current_operation_view(record: &OperationRecord) -> CurrentOperationView {
    CurrentOperationView {
        id: record.id,
        operation_type: record.operation_type,
        source: record.source,
        state: record.state,
    }
}

fn operation_status_from_record(record: &OperationRecord) -> OperationStatusView {
    OperationStatusView {
        id: record.id,
        operation_type: record.operation_type,
        service: record.service.clone(),
        source: record.source,
        state: record.state,
        created_at_ns: record.created_at_ns,
        started_at_ns: record.started_at_ns,
        completed_at_ns: record.completed_at_ns,
        result: operation_result(record),
        error: operation_error(record),
        merged_into: record.merged_into,
    }
}

fn operation_result(record: &OperationRecord) -> Option<String> {
    (record.state == OperationState::Completed)
        .then(|| record.result.clone())
        .flatten()
}

fn operation_error(record: &OperationRecord) -> Option<String> {
    matches!(
        record.state,
        OperationState::Failed | OperationState::Cancelled | OperationState::Aborted
    )
    .then(|| record.result.clone())
    .flatten()
}
