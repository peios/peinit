use crate::job::{JobEvent, JobType};

pub(super) fn origin_for_job(event: &JobEvent) -> String {
    let service = event.service.as_deref().unwrap_or("unknown");
    match event.job_type {
        JobType::PreExecHook => hook_origin(service, "ExecStartPre", event.hook_index),
        JobType::PostExecHook => hook_origin(service, "ExecStartPost", event.hook_index),
        JobType::ReloadHook => format!("{service}/ExecReload"),
        JobType::HealthCheck => format!("{service}/HealthCheck"),
        _ => service.to_string(),
    }
}

fn hook_origin(service: &str, label: &str, hook_index: Option<usize>) -> String {
    match hook_index {
        Some(index) => format!("{service}/{label}[{index}]"),
        None => format!("{service}/{label}"),
    }
}
