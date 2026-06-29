use peios::msgpack::Writer;

use crate::boundary::{BoundaryError, KmesEvent};
use crate::job::{JobEvent, JobEventDetail};

use super::super::labels::{job_state_label, job_type_label};
use super::super::payload::{
    finish_event, write_optional_i32_field, write_optional_str_field, write_optional_string_field,
    write_optional_u32_field, write_optional_u64_field, write_str_field, write_string_array_field,
    write_token_summary_field, write_uint_field,
};

pub fn encode_job_event(event: &JobEvent) -> Result<KmesEvent, BoundaryError> {
    match &event.detail {
        JobEventDetail::Created {
            image_path,
            identity,
            operation_id,
        } => encode_created_job_event(event, image_path, identity, *operation_id),
        JobEventDetail::Started {
            started_at_ns,
            pid,
            cgroup_id,
        } => encode_started_job_event(event, *started_at_ns, *pid, cgroup_id),
        JobEventDetail::Ended {
            ended_at_ns,
            duration_ns,
            exit_code,
            exit_signal,
            failure_cause,
        } => encode_ended_job_event(
            event,
            *ended_at_ns,
            *duration_ns,
            *exit_code,
            *exit_signal,
            failure_cause.as_deref(),
        ),
    }
}

fn encode_created_job_event(
    event: &JobEvent,
    image_path: &str,
    identity: &str,
    operation_id: Option<crate::ids::OperationId>,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(11);
    write_job_common(&mut writer, event);
    write_str_field(&mut writer, "image_path", image_path);
    write_str_field(&mut writer, "identity", identity);
    write_optional_string_field(
        &mut writer,
        "created_operation_id",
        operation_id.map(|id| id.to_string()),
    );
    finish_event("job.created", writer)
}

fn encode_started_job_event(
    event: &JobEvent,
    started_at_ns: u64,
    pid: u32,
    cgroup_id: &str,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(11);
    write_job_common(&mut writer, event);
    write_uint_field(&mut writer, "started_at_ns", started_at_ns);
    write_uint_field(&mut writer, "pid", u64::from(pid));
    write_str_field(&mut writer, "cgroup_id", cgroup_id);
    finish_event("job.started", writer)
}

fn encode_ended_job_event(
    event: &JobEvent,
    ended_at_ns: u64,
    duration_ns: u64,
    exit_code: Option<i32>,
    exit_signal: Option<i32>,
    failure_cause: Option<&str>,
) -> Result<KmesEvent, BoundaryError> {
    let mut writer = Writer::new();
    writer.write_map(22);
    write_job_common(&mut writer, event);
    write_optional_u32_field(&mut writer, "pid", event.pid);
    write_optional_i32_field(&mut writer, "pidfd", event.pidfd);
    write_str_field(&mut writer, "resolved_identity", &event.resolved_identity);
    write_str_field(&mut writer, "image_path", &event.image_path);
    write_string_array_field(&mut writer, "arguments", &event.arguments);
    write_uint_field(&mut writer, "created_at_ns", event.created_at_ns);
    write_optional_u64_field(&mut writer, "started_at_ns", event.started_at_ns);
    write_uint_field(&mut writer, "ended_at_ns", ended_at_ns);
    write_uint_field(&mut writer, "duration_ns", duration_ns);
    write_optional_i32_field(&mut writer, "exit_code", exit_code);
    write_optional_i32_field(&mut writer, "exit_signal", exit_signal);
    write_optional_str_field(&mut writer, "failure_cause", failure_cause);
    write_str_field(&mut writer, "cgroup_id", &event.cgroup_id);
    write_uint_field(&mut writer, "cgroup_generation", event.cgroup_generation);
    finish_event("job.ended", writer)
}

fn write_job_common(writer: &mut Writer, event: &JobEvent) {
    write_str_field(writer, "job_id", &event.job_id.to_string());
    write_optional_str_field(writer, "service", event.service.as_deref());
    write_str_field(writer, "type", job_type_label(event.job_type));
    write_str_field(writer, "state", job_state_label(event.state));
    write_optional_string_field(
        writer,
        "operation_id",
        event.operation_id.map(|id| id.to_string()),
    );
    write_token_summary_field(writer, "token_summary", &event.token_summary);
    write_str_field(writer, "token_identity", &event.token_summary.identity);
    write_str_field(writer, "final_state", job_state_label(event.state));
}
