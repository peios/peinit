use std::fs::File;
use std::os::fd::{FromRawFd, IntoRawFd};

use crate::boundary::{BoundaryError, EventdLogSink, LaunchedProcess, RealtimeClock};
use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::ids::{JobId, JobIdAllocator};
use crate::job::{JobEvent, JobRecord, JobType};
use crate::logging::{LogStream, ServiceLogRecord};
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource, RuntimeWorkPumpTurn,
};
use crate::security::TokenSummary;
use crate::supervisor::SupervisorPostStartHookLaunchDispatch;

use super::origin::origin_for_job;
use super::{
    DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES, RuntimeLogConfig, RuntimeLogPipeTurn,
    RuntimeServiceLogPipes,
};

#[test]
fn drains_complete_lines_into_pre_eventd_buffer() {
    let (read, mut write) = pipe_pair();
    use std::io::Write;
    write.write_all(b"one\ntwo\npartial").expect("write logs");
    drop(write);

    let mut pipes = RuntimeServiceLogPipes::new(RuntimeLogConfig {
        max_line_bytes: 100,
        read_bytes_per_event: 1024,
        pre_eventd_buffer_bytes: 4096,
        max_buffer_per_service_bytes: DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
    });
    let mut registrar = TestRegistrar::default();
    let event = job_event(JobType::ServiceMain);
    pipes
        .register_pipe(
            read.into_raw_fd(),
            "app".to_string(),
            LogStream::Stdout,
            &event,
            &mut registrar,
        )
        .expect("register pipe");

    let turn = pipes.process_pipe_event(registrar.calls[0], &mut ClockAt(10), &mut registrar);

    assert!(matches!(
        turn,
        RuntimeLogPipeTurn::Read { closed: true, .. }
    ));
    assert_eq!(
        pipes
            .buffered_records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two", "partial"],
    );
    assert_eq!(pipes.active_pipe_count(), 0);
}

#[test]
fn process_pipe_event_respects_per_turn_read_budget() {
    let (read, mut write) = pipe_pair();
    use std::io::Write;
    write.write_all(b"one\ntwo\n").expect("write logs");

    let mut pipes = RuntimeServiceLogPipes::new(RuntimeLogConfig {
        max_line_bytes: 100,
        read_bytes_per_event: 4,
        pre_eventd_buffer_bytes: 4096,
        max_buffer_per_service_bytes: DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
    });
    let mut registrar = TestRegistrar::default();
    let event = job_event(JobType::ServiceMain);
    let fd = read.into_raw_fd();
    pipes
        .register_pipe(
            fd,
            "app".to_string(),
            LogStream::Stdout,
            &event,
            &mut registrar,
        )
        .expect("register pipe");

    let first = pipes.process_pipe_event(fd, &mut ClockAt(10), &mut registrar);
    let second = pipes.process_pipe_event(fd, &mut ClockAt(11), &mut registrar);

    assert!(matches!(
        first,
        RuntimeLogPipeTurn::Read { closed: false, .. }
    ));
    assert!(matches!(
        second,
        RuntimeLogPipeTurn::Read { closed: false, .. }
    ));
    assert_eq!(
        pipes
            .buffered_records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two"],
    );
    assert_eq!(pipes.active_pipe_count(), 1);
    drop(write);

    let final_turn = pipes.process_pipe_event(fd, &mut ClockAt(12), &mut registrar);

    assert!(matches!(
        final_turn,
        RuntimeLogPipeTurn::Read { closed: true, .. }
    ));
    assert_eq!(pipes.active_pipe_count(), 0);
}

#[test]
fn derives_hook_origins() {
    assert_eq!(
        origin_for_job(&job_event(JobType::PreExecHook)),
        "app/ExecStartPre[0]",
    );
    assert_eq!(
        origin_for_job(&job_event(JobType::ReloadHook)),
        "app/ExecReload",
    );
    assert_eq!(
        origin_for_job(&job_event(JobType::PostExecHook)),
        "app/ExecStartPost[0]",
    );
    assert_eq!(
        origin_for_job(&job_event(JobType::HealthCheck)),
        "app/HealthCheck",
    );
}

#[test]
fn work_pump_registration_includes_post_start_hook_launches() {
    let (stdout_read, mut stdout_write) = pipe_pair();
    use std::io::Write;
    stdout_write.write_all(b"post\n").expect("write logs");
    drop(stdout_write);
    let stdout_fd = stdout_read.into_raw_fd();
    let turn = RuntimeWorkPumpTurn {
        post_hook_launches: vec![SupervisorPostStartHookLaunchDispatch {
            launch: LaunchCreatedJobDispatch {
                job_id: job_id(),
                process: LaunchedProcess {
                    pid: 7000,
                    pidfd: 70,
                    stdout_fd: Some(stdout_fd),
                    stderr_fd: None,
                    setup_status_fd: None,
                    cleanup_evidence: Vec::new(),
                },
                job_event: job_event(JobType::PostExecHook),
            },
        }],
        ..RuntimeWorkPumpTurn::default()
    };
    let mut pipes = RuntimeServiceLogPipes::new(RuntimeLogConfig {
        max_line_bytes: 100,
        read_bytes_per_event: 1024,
        pre_eventd_buffer_bytes: 4096,
        max_buffer_per_service_bytes: DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
    });
    let mut registrar = TestRegistrar::default();

    let registrations = pipes
        .register_work_pump_turn(&turn, &mut registrar)
        .expect("register post-start hook launch pipe");

    assert_eq!(
        registrations,
        vec![RuntimeEventSource::ServiceLogPipe { fd: stdout_fd }],
    );
    let _ = pipes.process_pipe_event(stdout_fd, &mut ClockAt(99), &mut registrar);
    let records = pipes.buffered_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].origin, "app/ExecStartPost[0]");
    assert_eq!(records[0].message, "post");
}

#[test]
fn retained_launches_register_output_pipes_when_runtime_starts() {
    let (stdout_read, _stdout_write) = pipe_pair();
    let (stderr_read, _stderr_write) = pipe_pair();
    let stdout_fd = stdout_read.into_raw_fd();
    let stderr_fd = stderr_read.into_raw_fd();
    let launch = LaunchCreatedJobDispatch {
        job_id: job_id(),
        process: LaunchedProcess {
            pid: 7000,
            pidfd: 70,
            stdout_fd: Some(stdout_fd),
            stderr_fd: Some(stderr_fd),
            setup_status_fd: None,
            cleanup_evidence: Vec::new(),
        },
        job_event: job_event(JobType::ServiceMain),
    };
    let mut pipes = RuntimeServiceLogPipes::default();
    let mut registrar = TestRegistrar::default();

    let registrations = pipes
        .register_retained_launches(&[launch], &mut registrar)
        .expect("register retained launch pipes");

    assert_eq!(
        registrations,
        vec![
            RuntimeEventSource::ServiceLogPipe { fd: stdout_fd },
            RuntimeEventSource::ServiceLogPipe { fd: stderr_fd },
        ],
    );
    assert_eq!(registrar.calls, vec![stdout_fd, stderr_fd]);
    assert_eq!(pipes.active_pipe_count(), 2);
}

#[test]
fn successful_eventd_flush_drains_buffer_in_order() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    pipes.pre_eventd.push(service_record("two"));
    let mut sink = FakeEventdSink::default();

    let flush = pipes.flush_to_eventd("/run/peios/eventd-log.sock", &mut sink);

    assert_eq!(flush.sent_records, 2);
    assert_eq!(flush.buffered_records, 0);
    assert!(flush.error.is_none());
    assert_eq!(
        sink.sent
            .iter()
            .map(|(_, record)| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two"],
    );
    assert!(pipes.buffered_records().is_empty());
}

#[test]
fn failed_eventd_flush_keeps_unsent_records_buffered() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    pipes.pre_eventd.push(service_record("two"));
    pipes.pre_eventd.push(service_record("three"));
    let mut sink = FakeEventdSink::fail_on_call(2);

    let flush = pipes.flush_to_eventd("/run/peios/eventd-log.sock", &mut sink);

    assert_eq!(flush.attempted_records, 2);
    assert_eq!(flush.sent_records, 1);
    assert_eq!(flush.buffered_records, 2);
    assert!(flush.error.is_some());
    assert_eq!(
        pipes
            .buffered_records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["two", "three"],
    );
}

#[test]
fn eventd_forwarding_waits_for_supervised_active_state() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    let mut sink = FakeEventdSink::default();

    let flush = pipes.sync_eventd_forwarding_with_sink(
        false,
        Some("/run/peios/eventd-log.sock"),
        &mut sink,
    );

    assert!(!flush.eventd_active);
    assert!(flush.socket_path_configured);
    assert_eq!(flush.attempted_records, 0);
    assert_eq!(flush.buffered_records, 1);
    assert!(sink.sent.is_empty());
    assert!(!pipes.eventd_forwarding_enabled());
}

#[test]
fn eventd_forwarding_replays_buffer_oldest_first_when_active() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    pipes.pre_eventd.push(service_record("two"));
    let mut sink = FakeEventdSink::default();

    let flush =
        pipes.sync_eventd_forwarding_with_sink(true, Some("/run/peios/eventd-log.sock"), &mut sink);

    assert!(flush.eventd_active);
    assert!(flush.socket_path_configured);
    assert_eq!(flush.sent_records, 2);
    assert!(flush.error.is_none());
    assert_eq!(
        sink.sent
            .iter()
            .map(|(_, record)| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two"],
    );
    assert!(pipes.buffered_records().is_empty());
    assert!(pipes.eventd_forwarding_enabled());
}

#[test]
fn eventd_forwarding_keeps_buffered_records_when_active_send_fails() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    pipes.pre_eventd.push(service_record("two"));
    let mut sink = FakeEventdSink::fail_on_call(1);

    let flush =
        pipes.sync_eventd_forwarding_with_sink(true, Some("/run/peios/eventd-log.sock"), &mut sink);

    assert!(flush.eventd_active);
    assert_eq!(flush.attempted_records, 1);
    assert_eq!(flush.sent_records, 0);
    assert_eq!(flush.buffered_records, 2);
    assert!(flush.error.is_some());
    assert_eq!(
        pipes
            .buffered_records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two"],
    );
    assert!(!pipes.eventd_forwarding_enabled());
}

#[test]
fn eventd_forwarding_buffers_again_while_eventd_is_inactive() {
    let mut pipes = RuntimeServiceLogPipes::default();
    let mut sink = FakeEventdSink::default();

    let active =
        pipes.sync_eventd_forwarding_with_sink(true, Some("/run/peios/eventd-log.sock"), &mut sink);
    assert!(active.error.is_none());
    assert!(pipes.eventd_forwarding_enabled());

    let inactive = pipes.sync_eventd_forwarding_with_sink(
        false,
        Some("/run/peios/eventd-log.sock"),
        &mut sink,
    );

    assert!(!inactive.eventd_active);
    assert!(inactive.socket_path_configured);
    assert!(!pipes.eventd_forwarding_enabled());
}

#[test]
fn active_eventd_receives_new_pipe_records_without_pre_eventd_buffering() {
    let (read, mut write) = pipe_pair();
    use std::io::Write;
    write.write_all(b"live\n").expect("write logs");
    drop(write);

    let mut pipes = RuntimeServiceLogPipes::default();
    let mut registrar = TestRegistrar::default();
    let event = job_event(JobType::ServiceMain);
    let fd = read.into_raw_fd();
    pipes
        .register_pipe(
            fd,
            "app".to_string(),
            LogStream::Stdout,
            &event,
            &mut registrar,
        )
        .expect("register pipe");
    let mut sink = FakeEventdSink::default();
    pipes.sync_eventd_forwarding_with_sink(true, Some("/run/peios/eventd.sock"), &mut sink);

    let turn = pipes.process_pipe_event_with_sink(fd, &mut ClockAt(20), &mut registrar, &mut sink);

    assert!(matches!(turn, RuntimeLogPipeTurn::Read { .. }));
    assert_eq!(
        sink.sent
            .iter()
            .map(|(_, record)| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["live"],
    );
    assert!(pipes.buffered_records().is_empty());
    assert!(pipes.eventd_forwarding_enabled());
}

#[test]
fn live_eventd_send_failure_buffers_unsent_records_and_disables_forwarding() {
    let (read, mut write) = pipe_pair();
    use std::io::Write;
    write.write_all(b"one\ntwo\n").expect("write logs");
    drop(write);

    let mut pipes = RuntimeServiceLogPipes::default();
    let mut registrar = TestRegistrar::default();
    let event = job_event(JobType::ServiceMain);
    let fd = read.into_raw_fd();
    pipes
        .register_pipe(
            fd,
            "app".to_string(),
            LogStream::Stdout,
            &event,
            &mut registrar,
        )
        .expect("register pipe");
    let mut setup_sink = FakeEventdSink::default();
    pipes.sync_eventd_forwarding_with_sink(true, Some("/run/peios/eventd.sock"), &mut setup_sink);
    let mut sink = FakeEventdSink::fail_on_call(2);

    pipes.process_pipe_event_with_sink(fd, &mut ClockAt(20), &mut registrar, &mut sink);

    assert_eq!(
        sink.sent
            .iter()
            .map(|(_, record)| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one"],
    );
    assert_eq!(
        pipes
            .buffered_records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["two"],
    );
    assert!(!pipes.eventd_forwarding_enabled());
}

#[derive(Default)]
struct TestRegistrar {
    calls: Vec<i32>,
}

impl RuntimeEventRegistrar for TestRegistrar {
    fn register_source(
        &mut self,
        fd: i32,
        _source: RuntimeEventSource,
    ) -> Result<(), RuntimeEventRegistrationError> {
        self.calls.push(fd);
        Ok(())
    }

    fn unregister_source(&mut self, _fd: i32) -> Result<(), RuntimeEventRegistrationError> {
        Ok(())
    }
}

struct ClockAt(u64);

impl RealtimeClock for ClockAt {
    fn realtime_ns(&mut self) -> Result<u64, BoundaryError> {
        Ok(self.0)
    }
}

fn pipe_pair() -> (File, File) {
    let mut fds = [0_i32; 2];
    let result = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    assert_eq!(
        result,
        0,
        "pipe2 failed: {}",
        std::io::Error::last_os_error()
    );
    unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
}

#[derive(Default)]
struct FakeEventdSink {
    sent: Vec<(String, ServiceLogRecord)>,
    fail_on_call: Option<usize>,
    calls: usize,
}

impl FakeEventdSink {
    fn fail_on_call(call: usize) -> Self {
        Self {
            fail_on_call: Some(call),
            ..Self::default()
        }
    }
}

impl EventdLogSink for FakeEventdSink {
    fn send_eventd_log_record(
        &mut self,
        socket_path: &str,
        record: &ServiceLogRecord,
    ) -> Result<(), BoundaryError> {
        self.calls += 1;
        if self.fail_on_call == Some(self.calls) {
            return Err(BoundaryError::EventdLog("eventd unavailable".to_string()));
        }
        self.sent.push((socket_path.to_string(), record.clone()));
        Ok(())
    }
}

fn job_event(job_type: JobType) -> JobEvent {
    JobEvent::created(&JobRecord {
        id: job_id(),
        service: Some("app".to_string()),
        job_type,
        hook_index: match job_type {
            JobType::PreExecHook | JobType::PostExecHook => Some(0),
            _ => None,
        },
        state: crate::job::JobState::Created,
        pid: None,
        pidfd: None,
        resolved_identity: "SYSTEM".to_string(),
        token_summary: TokenSummary::requested_identity("SYSTEM"),
        required_privileges: Vec::new(),
        image_path: "/sbin/app".to_string(),
        arguments: Vec::new(),
        environment: Vec::new(),
        working_directory: "/".to_string(),
        limit_nofile: None,
        limit_core: None,
        oom_score_adj: 0,
        created_at_ns: 1,
        started_at_ns: None,
        ended_at_ns: None,
        exit_code: None,
        exit_signal: None,
        failure_cause: None,
        cgroup_id: "app".to_string(),
        activation_generation: 1,
        cgroup_generation: 1,
        operation_id: None,
        attach_console: false,
    })
}

fn job_id() -> JobId {
    JobIdAllocator::new().allocate_batch(1, 1).expect("job id")[0]
}

fn service_record(message: &str) -> ServiceLogRecord {
    ServiceLogRecord::new("app", LogStream::Stdout, message, 1, Some(job_id()))
}
