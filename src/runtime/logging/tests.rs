use std::fs::File;
use std::os::fd::{FromRawFd, IntoRawFd};

use crate::boundary::{BoundaryError, EventdLogSink, LaunchedProcess, RealtimeClock};
use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::ids::{JobId, JobIdAllocator};
use crate::job::{JobEvent, JobRecord, JobType};
use crate::logging::{DEFAULT_PRE_EVENTD_BUFFER_BYTES, LogStream, ServiceLogRecord};
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
        eventd_log_datagram_bytes: crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES,
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
        eventd_log_datagram_bytes: crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES,
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
        eventd_log_datagram_bytes: crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES,
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

    let flush = pipes.flush_to_eventd("/run/services/eventd/eventd-log.sock", &mut sink);

    assert_eq!(flush.sent_records, 2);
    assert_eq!(flush.buffered_records, 0);
    assert!(flush.error.is_none());
    assert_eq!(sink.batch_sizes, vec![2]);
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
    force_single_record_datagrams(&mut pipes);
    let mut sink = FakeEventdSink::fail_on_call(2);

    let flush = pipes.flush_to_eventd("/run/services/eventd/eventd-log.sock", &mut sink);

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
        Some("/run/services/eventd/eventd-log.sock"),
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

    let flush = pipes.sync_eventd_forwarding_with_sink(
        true,
        Some("/run/services/eventd/eventd-log.sock"),
        &mut sink,
    );

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
fn repeated_eventd_sync_reuses_the_configured_path_allocation() {
    let mut pipes = RuntimeServiceLogPipes::default();
    let mut sink = FakeEventdSink::default();
    let path = "/run/services/eventd/eventd-log.sock";

    pipes.sync_eventd_forwarding_with_sink(true, Some(path), &mut sink);
    let first_path = pipes.eventd_socket_path.as_ref().expect("forwarding path");
    let first_ptr = first_path.as_ptr();
    let first_capacity = first_path.capacity();

    pipes.sync_eventd_forwarding_with_sink(true, Some(path), &mut sink);

    let second_path = pipes.eventd_socket_path.as_ref().expect("forwarding path");
    assert_eq!(second_path.as_ptr(), first_ptr);
    assert_eq!(second_path.capacity(), first_capacity);
}

/// `Machine\System\Init\PreEventdBuffer` reaches the buffer only through
/// `update_config`: the Linux runtime constructs its pipes with `Default`
/// before Phase 2 has read the registry, then syncs the effective config on
/// every turn. A capacity that did not follow the config left the key inert.
#[test]
fn update_config_applies_the_configured_pre_eventd_capacity() {
    let mut pipes = RuntimeServiceLogPipes::default();
    assert_eq!(
        pipes.pre_eventd.capacity_bytes(),
        DEFAULT_PRE_EVENTD_BUFFER_BYTES,
    );

    pipes.update_config(RuntimeLogConfig {
        pre_eventd_buffer_bytes: 4 * 1024 * 1024,
        ..RuntimeLogConfig::default()
    });

    assert_eq!(pipes.pre_eventd.capacity_bytes(), 4 * 1024 * 1024);
    assert_eq!(pipes.config().pre_eventd_buffer_bytes, 4 * 1024 * 1024);
}

/// A reload that lowers the capacity takes effect immediately, dropping the
/// oldest records rather than deferring until the buffer happens to drain.
#[test]
fn update_config_shrinking_capacity_evicts_buffered_records() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    pipes.pre_eventd.push(service_record("two"));
    pipes.pre_eventd.push(service_record("three"));

    pipes.update_config(RuntimeLogConfig {
        pre_eventd_buffer_bytes: 96,
        ..RuntimeLogConfig::default()
    });

    assert_eq!(pipes.pre_eventd.capacity_bytes(), 96);
    assert!(pipes.pre_eventd.used_bytes() <= 96);
    // Only the newest survives: these records carry a job id, so each is 16
    // bytes larger than the bare ones in the buffer's own tests and two no
    // longer fit in 96.
    assert_eq!(
        pipes
            .buffered_records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["three"],
    );
}

#[test]
fn eventd_forwarding_keeps_buffered_records_when_active_send_fails() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    pipes.pre_eventd.push(service_record("two"));
    let mut sink = FakeEventdSink::fail_on_call(1);

    let flush = pipes.sync_eventd_forwarding_with_sink(
        true,
        Some("/run/services/eventd/eventd-log.sock"),
        &mut sink,
    );

    assert!(flush.eventd_active);
    assert_eq!(flush.attempted_records, 2);
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

    let active = pipes.sync_eventd_forwarding_with_sink(
        true,
        Some("/run/services/eventd/eventd-log.sock"),
        &mut sink,
    );
    assert!(active.error.is_none());
    assert!(pipes.eventd_forwarding_enabled());

    let inactive = pipes.sync_eventd_forwarding_with_sink(
        false,
        Some("/run/services/eventd/eventd-log.sock"),
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
    pipes.sync_eventd_forwarding_with_sink(
        true,
        Some("/run/services/eventd/eventd.sock"),
        &mut sink,
    );

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
    pipes.sync_eventd_forwarding_with_sink(
        true,
        Some("/run/services/eventd/eventd.sock"),
        &mut setup_sink,
    );
    let mut sink = FakeEventdSink::fail_on_call(2);
    pipes.config.eventd_log_datagram_bytes = 1 + [service_record("one"), service_record("two")]
        .iter()
        .map(crate::logging::encoded_eventd_log_record_len)
        .max()
        .expect("records");

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

// PEI-357. §12.1 makes log ingestion deliberately lossy: eventd's SO_RCVBUF
// filling drops datagrams silently, and "peinit ... accepts that some MAY be
// dropped under load". A drop is therefore the designed outcome, not a
// transport failure.
//
// Treating it as one made peinit clear its socket path and push the whole rest
// of the batch back into the pre-eventd buffer — then the end-of-turn
// `sync_eventd_forwarding` re-enabled forwarding and replayed. So the system
// oscillated between forwarding and buffering under exactly the load the lossy
// design exists to absorb, and re-sent records eventd may already have held.
// The busier eventd got, the more peinit churned.
#[test]
fn a_dropped_live_datagram_keeps_forwarding_and_does_not_rebuffer() {
    let (read, mut write) = pipe_pair();
    use std::io::Write;
    write.write_all(b"one\ntwo\nthree\n").expect("write logs");
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
    pipes.sync_eventd_forwarding_with_sink(
        true,
        Some("/run/services/eventd/eventd.sock"),
        &mut setup_sink,
    );
    // One record per datagram, and the second one is dropped.
    let mut sink = FakeEventdSink::drop_on_call(2);
    pipes.config.eventd_log_datagram_bytes = 1 + [
        service_record("one"),
        service_record("two"),
        service_record("three"),
    ]
    .iter()
    .map(crate::logging::encoded_eventd_log_record_len)
    .max()
    .expect("records");

    pipes.process_pipe_event_with_sink(fd, &mut ClockAt(20), &mut registrar, &mut sink);

    // "two" is gone, and "three" went out behind it rather than being
    // buffered: one dropped datagram must not stop the ones after it.
    assert_eq!(
        sink.sent
            .iter()
            .map(|(_, record)| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "three"],
    );
    assert!(
        pipes.buffered_records().is_empty(),
        "a dropped datagram was pushed back into the pre-eventd buffer",
    );
    assert!(
        pipes.eventd_forwarding_enabled(),
        "a dropped datagram flipped peinit out of real-time forwarding",
    );
    // Dropping silently is not the same as dropping invisibly.
    assert_eq!(pipes.eventd_dropped_records(), 1);
}

// The replay path is the one place waiting beats dropping — the records are
// already buffered and the buffer is bounded, so the next turn tries again.
// What must not happen either way is the mode flip.
#[test]
fn a_dropped_replay_datagram_leaves_the_records_buffered_and_forwarding_on() {
    let mut pipes = RuntimeServiceLogPipes::default();
    pipes.pre_eventd.push(service_record("one"));
    pipes.pre_eventd.push(service_record("two"));
    force_single_record_datagrams(&mut pipes);
    let mut sink = FakeEventdSink::drop_on_call(1);

    let flush = pipes.sync_eventd_forwarding_with_sink(
        true,
        Some("/run/services/eventd/eventd.sock"),
        &mut sink,
    );

    assert_eq!(flush.sent_records, 0);
    assert!(flush.error.is_none(), "a drop is not a transport error");
    assert_eq!(pipes.buffered_records().len(), 2);
    assert!(pipes.eventd_forwarding_enabled());
}

/// TRM §11.1: a sink write that would block drops that line for the sink only,
/// counts it, and reports the first drop — once per job, however many follow.
///
/// The sink is a pipe shrunk to one page and filled, so every write the runtime
/// makes to it would block. Three lines are read: three are counted and the
/// first drop is reported. The sink is then drained and a fourth line fits: it
/// is written, the count stands, and nothing is reported. Two more with the
/// sink full again raise the count to five without a second report. Every line
/// reaches the record regardless, which is "for the sink only".
#[test]
fn a_sink_that_would_block_counts_each_dropped_line_and_reports_the_first() {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;

    let (stdout_read, mut stdout_write) = pipe_pair();
    // Non-blocking, as the launcher creates it: the pipe stays open across
    // reads here, and a blocking read end would wait for more output after
    // the last line rather than ending the read at EAGAIN.
    let flags = unsafe { libc::fcntl(stdout_read.as_raw_fd(), libc::F_GETFL) };
    assert_eq!(
        unsafe {
            libc::fcntl(
                stdout_read.as_raw_fd(),
                libc::F_SETFL,
                flags | libc::O_NONBLOCK,
            )
        },
        0,
    );
    let (mut sink_read, mut sink_write) = pipe_pair();
    let page = 4096;
    assert_eq!(
        unsafe { libc::fcntl(sink_write.as_raw_fd(), libc::F_SETPIPE_SZ, page) },
        page,
        "one-page sink",
    );
    sink_write
        .write_all(&vec![b'f'; page as usize])
        .expect("fill the sink");

    let event = job_event(JobType::Submitted);
    let job = event.job_id;
    let stdout_fd = stdout_read.into_raw_fd();
    let dispatch = crate::supervisor::SupervisorSubmittedLaunchDispatch {
        launch: LaunchCreatedJobDispatch {
            job_id: job,
            process: LaunchedProcess {
                pid: 7000,
                pidfd: 70,
                stdout_fd: Some(stdout_fd),
                stderr_fd: None,
                setup_status_fd: None,
                cleanup_evidence: Vec::new(),
            },
            job_event: event,
        },
        output_sink_fd: Some(sink_write.into_raw_fd()),
    };
    let mut pipes = RuntimeServiceLogPipes::default();
    let mut registrar = TestRegistrar::default();
    let mut registrations = Vec::new();
    pipes
        .register_submitted_launch(&dispatch, &mut registrar, &mut registrations)
        .expect("register submitted launch");
    assert_eq!(pipes.active_sink_count(), 1, "the sink was adopted");

    let mut read_turn = |pipes: &mut RuntimeServiceLogPipes, lines: &[u8]| {
        stdout_write.write_all(lines).expect("job output");
        match pipes.process_pipe_event(stdout_fd, &mut ClockAt(10), &mut registrar) {
            RuntimeLogPipeTurn::Read { output_dropped, .. } => output_dropped,
            other => panic!("expected a read, got {other:?}"),
        }
    };

    assert_eq!(
        read_turn(&mut pipes, b"one\ntwo\nthree\n"),
        Some(job),
        "the first drop is reported"
    );
    assert_eq!(
        pipes.sinks[&job].dropped, 3,
        "each line that would block is counted"
    );

    let mut drained = vec![0_u8; page as usize];
    sink_read.read_exact(&mut drained).expect("drain the sink");
    assert_eq!(
        read_turn(&mut pipes, b"four\n"),
        None,
        "a line that fits is not a drop"
    );
    assert_eq!(pipes.sinks[&job].dropped, 3, "and does not move the count");
    let mut written = [0_u8; 5];
    sink_read
        .read_exact(&mut written)
        .expect("the line reached the sink");
    assert_eq!(&written, b"four\n", "it went to the sink, untagged");

    // Full again: one page of filler written directly to the pipe the
    // runtime holds a copy of, through the reader's own view of its size.
    let sink_fd = pipes.sinks[&job].fd.as_raw_fd();
    let filler = vec![b'f'; page as usize];
    assert_eq!(
        unsafe { libc::write(sink_fd, filler.as_ptr().cast(), filler.len()) },
        page as isize,
        "refill the sink",
    );
    assert_eq!(
        read_turn(&mut pipes, b"five\nsix\n"),
        None,
        "only the first drop for a job is reported"
    );
    assert_eq!(
        pipes.sinks[&job].dropped, 5,
        "though every dropped line is counted"
    );

    assert_eq!(
        pipes
            .buffered_records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two", "three", "four", "five", "six"],
        "the record keeps every line, dropped from the sink or not",
    );
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
    batch_sizes: Vec<usize>,
    fail_on_call: Option<usize>,
    /// Calls that report the receive buffer full — the designed drop, not a
    /// transport failure.
    drop_on_call: Option<usize>,
    calls: usize,
}

impl FakeEventdSink {
    fn fail_on_call(call: usize) -> Self {
        Self {
            fail_on_call: Some(call),
            ..Self::default()
        }
    }

    fn drop_on_call(call: usize) -> Self {
        Self {
            drop_on_call: Some(call),
            ..Self::default()
        }
    }
}

impl EventdLogSink for FakeEventdSink {
    fn send_eventd_log_records(
        &mut self,
        socket_path: &str,
        records: &[ServiceLogRecord],
    ) -> Result<crate::boundary::EventdSendOutcome, BoundaryError> {
        self.calls += 1;
        if self.fail_on_call == Some(self.calls) {
            return Err(BoundaryError::EventdLog("eventd unavailable".to_string()));
        }
        if self.drop_on_call == Some(self.calls) {
            self.batch_sizes.push(records.len());
            return Ok(crate::boundary::EventdSendOutcome::Dropped);
        }
        self.batch_sizes.push(records.len());
        self.sent.extend(
            records
                .iter()
                .cloned()
                .map(|record| (socket_path.to_string(), record)),
        );
        Ok(crate::boundary::EventdSendOutcome::Sent)
    }
}

fn force_single_record_datagrams(pipes: &mut RuntimeServiceLogPipes) {
    pipes.config.eventd_log_datagram_bytes = 1 + pipes
        .pre_eventd
        .iter()
        .map(crate::logging::encoded_eventd_log_record_len)
        .max()
        .expect("buffered records");
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
        console_path: None,
    })
}

fn job_id() -> JobId {
    JobIdAllocator::new().allocate_batch(1, 1).expect("job id")[0]
}

fn service_record(message: &str) -> ServiceLogRecord {
    ServiceLogRecord::new("app", LogStream::Stdout, message, 1, Some(job_id()))
}
