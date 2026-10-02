//! The supported surface for programs that manage services from outside
//! peinit, such as Services Manager.
//!
//! It covers what to ask the control socket and how to read the answers
//! (PSPU §4), which right each command needs, what the generic rights mean on
//! a service, the descriptor a service has when none is given (§4.6), what a
//! command would come to against a service in each state (§10.3), and a
//! service's definition: its fields, and whether peinit would take it
//! (`Definition`).
//!
//! Everything here is what peinit itself uses, re-exported or read with
//! peinit's own labels, never a copy, so a client cannot drift from the
//! manager it talks to. The rest of the crate is the manager's own and is
//! not promised to anyone.
//!
//! peinit closes a control connection that has been idle for longer than
//! `Init\ConnectionTimeout`, so a client that waits between requests
//! connects for each batch of them rather than holding one open.

use serde_json::Value;

mod definition;

pub use definition::{Change, Definition, FIELDS, Problem, changes, problem};
pub use crate::control::client::{ControlClient, ControlClientError};
pub use crate::registry::{
    FieldGroup, FieldInfo, FieldKind, RawRegistryValue, RegistryValueType,
    ServiceRegistryDecodeError, TakesEffect, service_field,
};
pub use crate::control::lifecycle::{Admission, LifecycleCommand as Command, admission};
pub use crate::control::service_security::{
    DEFAULT_SERVICE_SECURITY_SDDL, SERVICE_GENERIC_MAPPING, ServiceAccess, ServiceGenericMapping,
};
pub use crate::control::socket::CONTROL_SOCKET_PATH;
pub use crate::operation::OperationState;
pub use crate::registry::SERVICES_ROOT_KEY;
pub use crate::service::runtime::{ServiceHealthStatus as Health, ServiceState as State};

use crate::control::wire::{
    operation_state_from_wire, service_health_from_wire, service_state_from_wire,
};

/// A service as `list` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub service: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub state: State,
    /// Why it is in that state, by the wire's name for it
    /// (`process_crash`), if it has a why.
    pub cause: Option<String>,
    /// Only a service with a health check has a health.
    pub health: Option<Health>,
    /// When the soonest of its calendar timers fires, if it has one armed.
    /// RFC 3339, in UTC.
    pub next_timer_at: Option<String>,
}

/// A service as `status` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub summary: Summary,
    /// What the service last said of itself (`STATUS=`).
    pub status_text: Option<String>,
    /// Its main process, while it has one.
    pub job: Option<Job>,
    /// The operation under way on it, if one is.
    pub operation: Option<CurrentOperation>,
    pub uptime_seconds: Option<u64>,
    /// Its definition has gone from the registry while it still runs.
    pub definition_removed: bool,
    pub warnings: Vec<Warning>,
    /// Its calendar timers, in the order of their schedules.
    pub timers: Vec<Timer>,
}

/// One calendar timer trigger of a service, as peinit has it armed (§9).
/// Times are RFC 3339, in UTC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timer {
    /// The calendar expression, as the definition gives it.
    pub schedule: String,
    /// The schedule's next occurrence.
    pub scheduled_at: Option<String>,
    /// When it will fire: that occurrence, delayed by whatever of
    /// `TimerJitter` was drawn for it.
    pub fires_at: Option<String>,
    /// When it last fired: this uptime, or for a persistent timer, the
    /// firing recorded before it.
    pub last_fired_at: Option<String>,
    /// Why it is not armed, when it is not: its schedule will not parse,
    /// or never comes round.
    pub not_armed: Option<String>,
}

/// A service's main process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub pid: Option<u32>,
    /// RFC 3339, in UTC.
    pub started_at: Option<String>,
    /// The principal it runs as, as a SID string.
    pub identity: String,
}

/// The operation under way on a service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentOperation {
    pub id: String,
    /// `start`, `stop`, `restart`, `reload` or `reset`.
    pub kind: String,
    /// What asked for it (`admin`, `boot`, `timer`…).
    pub source: String,
}

/// Something wrong with a service that is not its state: a sub-cgroup a
/// previous generation left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub path: String,
    pub kind: String,
    pub detected_at: String,
}

/// What a lifecycle command was answered with, not having been waited for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    /// The operation to follow with [`ControlClient::operation`]. None when
    /// the command did nothing, or did what it did at once.
    pub operation_id: Option<String>,
    pub state: State,
    pub cause: Option<String>,
    pub warnings: Vec<String>,
}

/// An operation as `operation-status` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: String,
    pub kind: String,
    pub service: String,
    pub source: String,
    pub state: OperationState,
    /// What it came to, once it completed.
    pub result: Option<String>,
    /// Why not, once it failed, was cancelled or was aborted.
    pub error: Option<String>,
    /// The operation it was merged into, if it was.
    pub merged_into: Option<String>,
}

impl Operation {
    /// Whether it is over, one way or another.
    pub fn finished(&self) -> bool {
        !matches!(
            self.state,
            OperationState::Pending | OperationState::Running
        )
    }
}

/// Why a request came to nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The socket could not be reached, or the conversation broke.
    Unreachable(String),
    /// peinit refused: its code (`ACCESS_DENIED`, `INVALID_STATE`…) and
    /// its message.
    Refused { code: String, message: String },
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(why) => f.write_str(why),
            Self::Refused { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

impl std::error::Error for Failure {}

impl From<ControlClientError> for Failure {
    fn from(error: ControlClientError) -> Self {
        Self::Unreachable(error.to_string())
    }
}

impl ControlClient {
    /// Every service the caller may query. The rest are left out without a
    /// word, which is the point (§10.2).
    pub fn services(&mut self) -> Result<Vec<Summary>, Failure> {
        let answer = answered(self.service_list()?)?;
        answer["services"]
            .as_array()
            .ok_or_else(|| garbled("a list with no services"))?
            .iter()
            .map(summary)
            .collect()
    }

    pub fn status_of(&mut self, service: &str) -> Result<Status, Failure> {
        let answer = answered(self.service_status(service)?)?;
        let mut status = Status {
            summary: summary(&answer)?,
            status_text: text(&answer, "status_text"),
            job: match &answer["current_job"] {
                Value::Null => None,
                job => Some(Job {
                    id: text(job, "id").unwrap_or_default(),
                    pid: job["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok()),
                    started_at: text(job, "started_at"),
                    identity: text(job, "identity").unwrap_or_default(),
                }),
            },
            operation: match &answer["current_operation"] {
                Value::Null => None,
                operation => Some(CurrentOperation {
                    id: text(operation, "id").unwrap_or_default(),
                    kind: text(operation, "type").unwrap_or_default(),
                    source: text(operation, "source").unwrap_or_default(),
                }),
            },
            uptime_seconds: answer["uptime_seconds"].as_u64(),
            definition_removed: answer["definition_removed"].as_bool().unwrap_or(false),
            warnings: answer["warnings"]
                .as_array()
                .map(|warnings| {
                    warnings
                        .iter()
                        .map(|warning| Warning {
                            path: text(warning, "path").unwrap_or_default(),
                            kind: text(warning, "type").unwrap_or_default(),
                            detected_at: text(warning, "detected_at").unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            timers: answer["timers"]
                .as_array()
                .map(|timers| {
                    timers
                        .iter()
                        .map(|timer| Timer {
                            schedule: text(timer, "schedule").unwrap_or_default(),
                            scheduled_at: text(timer, "scheduled_at"),
                            fires_at: text(timer, "fires_at"),
                            last_fired_at: text(timer, "last_fired_at"),
                            not_armed: text(timer, "not_armed"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        // As `list` would have it. peinit's times are all one width, in UTC,
        // so the soonest is the least.
        status.summary.next_timer_at = status
            .timers
            .iter()
            .filter_map(|timer| timer.fires_at.clone())
            .min();
        Ok(status)
    }

    /// Asks for `command` on `service` and does not wait for it: what it
    /// started is followed with [`ControlClient::operation`].
    pub fn command(&mut self, command: Command, service: &str) -> Result<Accepted, Failure> {
        let answer = answered(self.service_command(wire_command(command), service, false)?)?;
        Ok(Accepted {
            operation_id: text(&answer, "operation_id"),
            state: state(&answer)?,
            cause: text(&answer, "cause"),
            warnings: strings(&answer["warnings"]),
        })
    }

    pub fn operation(&mut self, id: &str) -> Result<Operation, Failure> {
        let answer = answered(self.operation_status(id)?)?;
        let operation = &answer["operation"];
        Ok(Operation {
            id: text(operation, "id").unwrap_or_default(),
            kind: text(operation, "type").unwrap_or_default(),
            service: text(operation, "service").unwrap_or_default(),
            source: text(operation, "source").unwrap_or_default(),
            state: operation["state"]
                .as_str()
                .and_then(operation_state_from_wire)
                .ok_or_else(|| garbled("an operation in no state it knows"))?,
            result: text(operation, "result"),
            error: text(operation, "error"),
            merged_into: text(operation, "merged_into"),
        })
    }
}

fn wire_command(command: Command) -> &'static str {
    match command {
        Command::Start => "start",
        Command::Stop => "stop",
        Command::Restart => "restart",
        Command::Reload => "reload",
        Command::Reset => "reset",
    }
}

/// The answer, if peinit did not refuse.
fn answered(response: crate::control::client::ControlResponse) -> Result<Value, Failure> {
    if response.is_ok() {
        Ok(response.value().clone())
    } else {
        Err(Failure::Refused {
            code: response.error_code().unwrap_or("").to_string(),
            message: response.error_message().unwrap_or("").to_string(),
        })
    }
}

fn summary(value: &Value) -> Result<Summary, Failure> {
    Ok(Summary {
        service: text(value, "service").ok_or_else(|| garbled("a service with no name"))?,
        display_name: text(value, "display_name"),
        description: text(value, "description"),
        state: state(value)?,
        cause: text(value, "cause"),
        health: value["health"].as_str().and_then(service_health_from_wire),
        next_timer_at: text(value, "next_timer_at"),
    })
}

fn state(value: &Value) -> Result<State, Failure> {
    value["state"]
        .as_str()
        .and_then(service_state_from_wire)
        .ok_or_else(|| garbled("a service in no state it knows"))
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(ToOwned::to_owned)
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn garbled(what: &str) -> Failure {
    Failure::Unreachable(format!("peinit answered with {what}"))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    use super::*;
    use crate::control::query::ServiceListItem;
    use crate::control::wire::{
        ControlErrorCode, control_error_response_line, control_lifecycle_ack_response_line,
        control_list_response_line,
    };
    use crate::ids::OperationId;
    use crate::service::runtime::TransitionCause;

    /// A control socket that answers each request with the next line given.
    fn answering(name: &str, lines: Vec<Vec<u8>>) -> ControlClient {
        let path =
            std::env::temp_dir().join(format!("peinit-client-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind");
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut stream = stream;
            for line in lines {
                let mut request = String::new();
                reader.read_line(&mut request).expect("read request");
                stream.write_all(&line).expect("answer");
            }
        });
        let client = ControlClient::connect_path(&path).expect("connect");
        let _ = std::fs::remove_file(&path);
        client
    }

    /// What peinit writes for `list` is read back as it was meant.
    #[test]
    fn a_list_is_read_as_peinit_writes_it() {
        let items = [
            ServiceListItem {
                service: "timed".into(),
                display_name: Some("Time client".into()),
                description: None,
                state: State::Active,
                cause: Some(TransitionCause::DependencyStart),
                health: None,
                definition_removed: false,
                next_timer_ns: None,
            },
            ServiceListItem {
                service: "sshd".into(),
                display_name: None,
                description: Some("Secure shell".into()),
                state: State::Failed,
                cause: Some(TransitionCause::ProcessCrash),
                health: Some(Health::Unhealthy),
                definition_removed: false,
                next_timer_ns: Some(1_717_200_000_000_000_000),
            },
        ];
        let mut client = answering("list", vec![control_list_response_line(&items).unwrap()]);
        let services = client.services().unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].service, "timed");
        assert_eq!(services[0].display_name.as_deref(), Some("Time client"));
        assert_eq!(services[0].state, State::Active);
        assert_eq!(services[0].cause.as_deref(), Some("dependency_start"));
        assert_eq!(services[1].state, State::Failed);
        assert_eq!(services[1].health, Some(Health::Unhealthy));
        assert_eq!(services[1].description.as_deref(), Some("Secure shell"));
        assert_eq!(services[0].next_timer_at, None);
        assert_eq!(
            services[1].next_timer_at.as_deref(),
            Some("2024-06-01T00:00:00.000000000Z")
        );
    }

    /// A status's timers are read back, the soonest of them standing for
    /// the service as `list` would have it, and one not armed says why.
    #[test]
    fn a_status_is_read_with_its_timers() {
        use crate::control::query::{ServiceStatusView, ServiceTimerArming, ServiceTimerView};
        use crate::control::wire::{ControlResponseTimeProjection, control_status_response_line};

        let armed = |schedule: &str, fires_ns: u64| ServiceTimerView {
            schedule: schedule.into(),
            arming: ServiceTimerArming::Armed {
                scheduled_ns: fires_ns,
                fires_ns,
                last_fired_ns: Some(1_717_000_000_000_000_000),
            },
        };
        let view = ServiceStatusView {
            service: "backup".into(),
            display_name: None,
            description: None,
            state: State::Inactive,
            cause: None,
            generation: 1,
            status_text: None,
            health: None,
            definition_removed: false,
            current_job: None,
            current_operation: None,
            warnings: Vec::new(),
            lifecycle_warnings: Vec::new(),
            timers: vec![
                armed("*-*-* 03:00:00", 1_717_210_800_000_000_000),
                armed("hourly", 1_717_203_600_000_000_000),
                ServiceTimerView {
                    schedule: "*-02-30".into(),
                    arming: ServiceTimerArming::NotArmed {
                        reason: "calendar expression has no future occurrence".into(),
                    },
                },
            ],
        };
        let line =
            control_status_response_line(&view, ControlResponseTimeProjection::new(0, 0)).unwrap();
        let status = answering("status", vec![line]).status_of("backup").unwrap();
        assert_eq!(status.timers.len(), 3);
        assert_eq!(status.timers[0].schedule, "*-*-* 03:00:00");
        assert_eq!(
            status.timers[0].fires_at.as_deref(),
            Some("2024-06-01T03:00:00.000000000Z")
        );
        assert_eq!(
            status.timers[0].last_fired_at.as_deref(),
            Some("2024-05-29T16:26:40.000000000Z")
        );
        assert_eq!(
            status.summary.next_timer_at.as_deref(),
            Some("2024-06-01T01:00:00.000000000Z")
        );
        assert_eq!(status.timers[2].fires_at, None);
        assert_eq!(
            status.timers[2].not_armed.as_deref(),
            Some("calendar expression has no future occurrence")
        );
    }

    /// A command not waited for comes back with the operation to follow,
    /// and a refusal comes back as one.
    #[test]
    fn a_command_is_followed_by_its_operation_and_a_refusal_is_one() {
        let id = OperationId::parse_canonical_str("01890a5d-ac96-774b-bcce-b302099a8057").unwrap();
        let mut client = answering(
            "command",
            vec![
                control_lifecycle_ack_response_line(
                    Some(id),
                    "timed",
                    State::Starting,
                    Some(TransitionCause::ExplicitStart),
                    &[],
                )
                .unwrap(),
                control_error_response_line(ControlErrorCode::AccessDenied, "access denied")
                    .unwrap(),
            ],
        );
        let accepted = client.command(Command::Start, "timed").unwrap();
        assert_eq!(
            accepted.operation_id.as_deref(),
            Some("01890a5d-ac96-774b-bcce-b302099a8057")
        );
        assert_eq!(accepted.state, State::Starting);
        assert_eq!(
            client.command(Command::Stop, "timed"),
            Err(Failure::Refused {
                code: "ACCESS_DENIED".into(),
                message: "access denied".into()
            })
        );
    }

    /// Every state, health and operation state peinit writes is one a client
    /// reads back.
    #[test]
    fn every_label_written_is_read_back() {
        use crate::control::wire::{
            operation_state_from_wire, service_health_from_wire, service_state_from_wire,
        };
        for state in [
            State::Inactive,
            State::Starting,
            State::Active,
            State::Reloading,
            State::Stopping,
            State::Completed,
            State::Backoff,
            State::Failed,
            State::Abandoned,
            State::Skipped,
        ] {
            let line = control_lifecycle_ack_response_line(None, "x", state, None, &[]).unwrap();
            let value: Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(
                value["state"].as_str().and_then(service_state_from_wire),
                Some(state)
            );
        }
        assert_eq!(
            service_health_from_wire("unhealthy"),
            Some(Health::Unhealthy)
        );
        assert_eq!(
            operation_state_from_wire("merged"),
            Some(OperationState::Merged)
        );
        assert_eq!(service_state_from_wire("sleeping"), None);
    }

    /// A button for a command is worth showing only where the command acts,
    /// and only to whoever has the right it needs.
    #[test]
    fn what_a_command_comes_to_and_what_it_needs() {
        assert_eq!(admission(Command::Start, State::Inactive), Admission::Acts);
        assert_eq!(admission(Command::Start, State::Active), Admission::Already);
        assert_eq!(
            admission(Command::Stop, State::Inactive),
            Admission::Nothing
        );
        assert_eq!(
            admission(Command::Reload, State::Inactive),
            Admission::Refused
        );
        assert_eq!(admission(Command::Reset, State::Failed), Admission::Acts);
        assert_eq!(ServiceAccess::for_command(Command::Restart).bits(), 0x6);
        assert_eq!(
            ServiceAccess::for_command(Command::Reset),
            ServiceAccess::STOP
        );
        assert_eq!(SERVICE_GENERIC_MAPPING.write, 0xe);
    }
}
