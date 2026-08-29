use std::io::{self, Write};

use serde_json::Value;

use crate::shutdown::ShutdownKind;

use super::command::{Command, OutputMode, ServiceAction};
use super::execute::CliResponse;

pub fn write_response(
    out: &mut dyn Write,
    command: &Command,
    response: &CliResponse,
    output: OutputMode,
) -> io::Result<()> {
    match output {
        OutputMode::Json => writeln!(out, "{}", response.raw_json()),
        OutputMode::Human => write_human_response(out, command, response.value()),
    }
}

pub fn write_server_error(
    err: &mut dyn Write,
    response: &CliResponse,
    output: OutputMode,
) -> io::Result<()> {
    if output == OutputMode::Json {
        return Ok(());
    }
    let code = response.error_code().unwrap_or("ERROR");
    let message = response.error_message().unwrap_or("control request failed");
    writeln!(err, "svctl: {code}: {message}")
}

fn write_human_response(out: &mut dyn Write, command: &Command, value: &Value) -> io::Result<()> {
    match command {
        Command::List => write_list(out, value),
        Command::Status { .. } => write_status(out, value),
        Command::OperationStatus { .. } => write_operation_status(out, value),
        Command::ReloadConfig => write_reload_config(out, value),
        Command::Shutdown { kind } => writeln!(out, "shutdown requested: {}", shutdown_name(*kind)),
        Command::Service {
            action,
            service,
            wait,
        } => write_service_ack(out, *action, service, *wait, value),
        Command::JobList { .. } => write_job_list(out, value),
        Command::JobStatus { .. }
        | Command::JobStop { .. }
        | Command::JobSubmit { .. }
        | Command::JobWait { .. }
        | Command::JobSignal { .. } => write_job_view(out, value.get("job").unwrap_or(value)),
    }
}

fn write_job_list(out: &mut dyn Write, value: &Value) -> io::Result<()> {
    let jobs = value
        .get("jobs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut rows = Vec::with_capacity(jobs.len() + 1);
    rows.push(vec![
        "JOB".to_string(),
        "STATE".to_string(),
        "SUBMITTER".to_string(),
        "IDENTITY".to_string(),
        "PROGRESS".to_string(),
        "DESCRIPTION".to_string(),
    ]);
    for job in jobs {
        rows.push(vec![
            str_field(job, "id").unwrap_or("").to_string(),
            str_field(job, "state").unwrap_or("").to_string(),
            str_field(job, "submitter").unwrap_or("").to_string(),
            str_field(job, "identity").unwrap_or("").to_string(),
            progress_text(job.get("progress")).unwrap_or_else(|| "-".to_string()),
            str_field(job, "description").unwrap_or("").to_string(),
        ]);
    }
    write_table(out, &rows)
}

/// The job view (PSPU §7.7), one field per line.
fn write_job_view(out: &mut dyn Write, job: &Value) -> io::Result<()> {
    let id = str_field(job, "id").unwrap_or("unknown");
    let state = str_field(job, "state").unwrap_or("unknown");
    writeln!(out, "job {id}: {state}")?;
    write_optional_line(out, "cause", str_field(job, "cause"))?;
    write_optional_line(
        out,
        "description",
        str_field(job, "description").filter(|text| !text.is_empty()),
    )?;
    write_optional_line(out, "image", str_field(job, "image_path"))?;
    if let Some(pid) = job.get("pid").and_then(Value::as_i64) {
        writeln!(out, "pid: {pid}")?;
    }
    write_optional_line(out, "submitter", str_field(job, "submitter"))?;
    write_optional_line(out, "identity", str_field(job, "identity"))?;
    if let Some(session) = job.get("logon_session").and_then(Value::as_u64) {
        writeln!(out, "logon session: {session:#x}")?;
    }
    if let Some(ready) = job.get("ready").and_then(Value::as_bool) {
        writeln!(out, "ready: {}", if ready { "yes" } else { "no" })?;
    }
    write_optional_line(out, "status", str_field(job, "status_text"))?;
    write_optional_line(
        out,
        "progress",
        progress_text(job.get("progress")).as_deref(),
    )?;
    if let Some(code) = job.get("exit_code").and_then(Value::as_i64) {
        writeln!(out, "exit code: {code}")?;
    }
    if let Some(signal) = job.get("exit_signal").and_then(Value::as_i64) {
        writeln!(out, "exit signal: {signal}")?;
    }
    write_optional_line(out, "created", str_field(job, "created_at"))?;
    write_optional_line(out, "started", str_field(job, "started_at"))?;
    write_optional_line(out, "ended", str_field(job, "ended_at"))
}

/// `N`, `N/?`, or `N/T`, with the unit when one was given.
fn progress_text(value: Option<&Value>) -> Option<String> {
    let progress = value.filter(|value| !value.is_null())?;
    let current = progress.get("current").and_then(Value::as_u64)?;
    let bounded = progress
        .get("bounded")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut text = match (bounded, progress.get("total").and_then(Value::as_u64)) {
        (true, Some(total)) => format!("{current}/{total}"),
        (true, None) => format!("{current}/?"),
        (false, _) => current.to_string(),
    };
    if let Some(unit) = str_field(progress, "unit") {
        text.push(' ');
        text.push_str(unit);
    }
    Some(text)
}

fn write_service_ack(
    out: &mut dyn Write,
    action: ServiceAction,
    requested_service: &str,
    wait: bool,
    value: &Value,
) -> io::Result<()> {
    let service = str_field(value, "service").unwrap_or(requested_service);
    let state = str_field(value, "state").unwrap_or("unknown");
    write!(out, "{service}: {state}")?;
    if let Some(mode) = str_field(value, "mode") {
        write!(out, " ({mode})")?;
    }
    writeln!(out)?;
    if let Some(operation_id) = str_field(value, "operation_id") {
        writeln!(out, "operation: {operation_id}")?;
    }
    if let Some(cause) = str_field(value, "cause") {
        writeln!(out, "cause: {cause}")?;
    }
    if action.accepts_wait() {
        writeln!(out, "wait: {}", if wait { "yes" } else { "no" })?;
    }
    write_warnings(out, value)?;
    Ok(())
}

fn write_status(out: &mut dyn Write, value: &Value) -> io::Result<()> {
    let service = str_field(value, "service").unwrap_or("unknown");
    let state = str_field(value, "state").unwrap_or("unknown");
    writeln!(out, "{service}: {state}")?;
    write_optional_line(out, "cause", str_field(value, "cause"))?;
    write_optional_line(out, "health", str_field(value, "health"))?;
    write_optional_line(out, "status", str_field(value, "status_text"))?;
    if let Some(uptime) = value.get("uptime_seconds").and_then(Value::as_u64) {
        writeln!(out, "uptime: {uptime}s")?;
    }
    if value
        .get("definition_removed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        writeln!(out, "definition: removed")?;
    }
    write_current_job(out, value.get("current_job"))?;
    write_current_operation(out, value.get("current_operation"))?;
    write_warnings(out, value)?;
    Ok(())
}

fn write_current_job(out: &mut dyn Write, value: Option<&Value>) -> io::Result<()> {
    let Some(job) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let id = str_field(job, "id").unwrap_or("unknown");
    let job_type = str_field(job, "type").unwrap_or("unknown");
    writeln!(out, "job: {id} ({job_type})")?;
    if let Some(pid) = job.get("pid").and_then(Value::as_i64) {
        writeln!(out, "pid: {pid}")?;
    }
    write_optional_line(out, "started", str_field(job, "started_at"))?;
    write_optional_line(out, "identity", str_field(job, "identity"))
}

fn write_current_operation(out: &mut dyn Write, value: Option<&Value>) -> io::Result<()> {
    let Some(operation) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let id = str_field(operation, "id").unwrap_or("unknown");
    let operation_type = str_field(operation, "type").unwrap_or("unknown");
    let source = str_field(operation, "source").unwrap_or("unknown");
    writeln!(out, "operation: {id} ({operation_type}, {source})")
}

fn write_list(out: &mut dyn Write, value: &Value) -> io::Result<()> {
    let services = value
        .get("services")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut rows = Vec::with_capacity(services.len() + 1);
    rows.push(vec![
        "SERVICE".to_string(),
        "STATE".to_string(),
        "HEALTH".to_string(),
        "CAUSE".to_string(),
    ]);
    for service in services {
        rows.push(vec![
            str_field(service, "service").unwrap_or("").to_string(),
            str_field(service, "state").unwrap_or("").to_string(),
            str_field(service, "health").unwrap_or("-").to_string(),
            str_field(service, "cause").unwrap_or("-").to_string(),
        ]);
    }
    write_table(out, &rows)
}

fn write_operation_status(out: &mut dyn Write, value: &Value) -> io::Result<()> {
    let operation = value.get("operation").unwrap_or(value);
    let id = str_field(operation, "id").unwrap_or("unknown");
    let state = str_field(operation, "state").unwrap_or("unknown");
    writeln!(out, "operation {id}: {state}")?;
    write_optional_line(out, "type", str_field(operation, "type"))?;
    write_optional_line(out, "service", str_field(operation, "service"))?;
    write_optional_line(out, "source", str_field(operation, "source"))?;
    write_optional_line(out, "result", str_field(operation, "result"))?;
    write_optional_line(out, "merged_into", str_field(operation, "merged_into"))?;
    write_optional_line(out, "error", str_field(operation, "error"))?;
    write_optional_line(out, "requested", str_field(operation, "requested_at"))?;
    write_optional_line(out, "started", str_field(operation, "started_at"))?;
    write_optional_line(out, "completed", str_field(operation, "completed_at"))
}

fn write_reload_config(out: &mut dyn Write, value: &Value) -> io::Result<()> {
    writeln!(out, "configuration reloaded")?;
    if let Some(summary) = value.get("summary").and_then(Value::as_object) {
        for key in [
            "added",
            "updated",
            "restored",
            "marked_removed",
            "discarded",
        ] {
            let count = summary
                .get(key)
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            writeln!(out, "{key}: {count}")?;
        }
    }
    write_warnings(out, value)
}

fn write_warnings(out: &mut dyn Write, value: &Value) -> io::Result<()> {
    let Some(warnings) = value.get("warnings").and_then(Value::as_array) else {
        return Ok(());
    };
    if warnings.is_empty() {
        return Ok(());
    }
    writeln!(out, "warnings:")?;
    for warning in warnings {
        match warning {
            Value::String(message) => writeln!(out, "  {message}")?,
            Value::Object(_) => writeln!(out, "  {}", compact_json(warning))?,
            _ => writeln!(out, "  {warning}")?,
        }
    }
    Ok(())
}

fn write_table(out: &mut dyn Write, rows: &[Vec<String>]) -> io::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0_usize; column_count];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.len());
        }
    }
    for row in rows {
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                write!(out, "  ")?;
            }
            let cell = row.get(index).map(String::as_str).unwrap_or("");
            if index + 1 == column_count {
                write!(out, "{cell}")?;
            } else {
                write!(out, "{cell:<width$}")?;
            }
        }
        writeln!(out)?;
    }
    Ok(())
}

fn write_optional_line(out: &mut dyn Write, label: &str, value: Option<&str>) -> io::Result<()> {
    if let Some(value) = value {
        writeln!(out, "{label}: {value}")?;
    }
    Ok(())
}

fn str_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn shutdown_name(kind: ShutdownKind) -> &'static str {
    match kind {
        ShutdownKind::Poweroff => "poweroff",
        ShutdownKind::Reboot => "reboot",
        ShutdownKind::Halt => "halt",
    }
}
