use crate::control::connection::{ControlConnectionRecord, ControlConnectionTable};
use crate::control::socket::{ControlSocketRead, ControlSocketWrite};
use crate::runtime::{
    RuntimeControlLimits, RuntimeEventSource, RuntimeShutdownEventSources,
    RuntimeShutdownEventTurn, RuntimeShutdownLoopContext, RuntimeWorkPumpConfig,
    process_runtime_shutdown_sources_with_registry,
};
use crate::supervisor::SupervisorControlFrameTurn;

use super::super::super::fixture::shutdown_fixture;
use super::super::support::{
    AllowAccessChecker, FakeAcceptedConnection, FakeBootAttemptCounter, FakeChildReaper,
    FakeControlListener, FakeDeadlineTimer, FakeNotifySource, FakeRegistrar, FakeSignalSource,
    RuntimeFinalizer, control_peer,
};
use crate::supervisor::tests::{
    ScriptedClock, StaticRegistry, TestProcessController, TestProcessLauncher, TestTokenProvider,
    alive_service,
};

#[test]
fn runtime_sources_reject_reload_config_during_shutdown() {
    let mut supervisor = shutdown_fixture();
    let mut registry = StaticRegistry::services(vec![alive_service("fresh")]);
    let mut signal = FakeSignalSource::would_block();
    let mut child_reaper = FakeChildReaper::empty();
    let mut notify = FakeNotifySource::empty();
    let mut listener = FakeControlListener::default();
    let mut connections = ControlConnectionTable::new(4);
    connections
        .admit(
            47,
            ControlConnectionRecord::new(
                FakeAcceptedConnection::with_io(
                    47,
                    "admin",
                    [ControlSocketRead::Bytes(
                        b"{\"command\":\"reload-config\"}\n".to_vec(),
                    )],
                    [ControlSocketWrite::Complete],
                ),
                control_peer("admin"),
            ),
        )
        .expect("seed connection");
    let mut deadline_timer = FakeDeadlineTimer::would_block();
    let mut lifecycle_timer = FakeDeadlineTimer::would_block();
    let mut power_button = super::super::support::FakePowerButtonSource::would_block();
    let mut log_pipes = crate::runtime::RuntimeServiceLogPipes::default();
    let mut filesystem_check_reader =
        crate::supervisor::tests::TestFilesystemCheckReader::default();
    let mut clock = ScriptedClock::new([123]);
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(
            crate::shutdown::ShutdownKind::Reboot,
            &mut controller,
            2_000_000_000,
        )
        .expect("begin shutdown");
    let mut finalizer = RuntimeFinalizer::default();
    let mut access = AllowAccessChecker::default();
    let mut registrar = FakeRegistrar::default();
    let mut boot_attempt_counter = FakeBootAttemptCounter::default();
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(Vec::new());
    let mut filesystem_check_launcher =
        crate::supervisor::tests::TestFilesystemCheckLauncher::default();

    let turn = process_runtime_shutdown_sources_with_registry(
        &mut supervisor,
        vec![RuntimeEventSource::ControlConnection { fd: 47 }],
        &mut RuntimeShutdownEventSources {
            signal_source: &mut signal,
            child_reaper: &mut child_reaper,
            notify_source: &mut notify,
            control_listener: &mut listener,
            control_connections: &mut connections,
            deadline_timer: &mut deadline_timer,
            lifecycle_timer: &mut lifecycle_timer,
            power_button_source: &mut power_button,
            filesystem_check_reader: &mut filesystem_check_reader,
            log_pipes: &mut log_pipes,
        },
        Some(&mut registry),
        None,
        RuntimeShutdownLoopContext {
            clock: &mut clock,
            controller: &mut controller,
            finalizer: &mut finalizer,
            access_checker: &mut access,
            registrar: &mut registrar,
            token_provider: &mut tokens,
            process_launcher: &mut launcher,
            filesystem_check_launcher: &mut filesystem_check_launcher,
            boot_attempt_counter: &mut boot_attempt_counter,
            control_security: &crate::control::system::ControlSecurityDescriptor::Default,
            max_events: 8,
            control_limits: RuntimeControlLimits::new(
                1024,
                crate::control::socket::DEFAULT_MAX_REQUEST_SIZE_BYTES,
                crate::control::socket::DEFAULT_CONNECTION_TIMEOUT_SECS,
            ),
            work_pump: RuntimeWorkPumpConfig::default(),
        },
    )
    .expect("runtime sources");

    let RuntimeShutdownEventTurn::ControlConnection {
        supervisor: connection_turn,
        ..
    } = &turn.turns[0]
    else {
        panic!("expected control connection turn");
    };
    let SupervisorControlFrameTurn::CommandRejected {
        response_line,
        error,
        ..
    } = connection_turn
        .turn
        .frame
        .as_ref()
        .expect("frame")
        .frame
        .clone()
    else {
        panic!("expected rejected reload-config command during shutdown");
    };
    let json = response_json(&response_line);
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "INVALID_STATE");
    assert_eq!(json["message"], "command rejected during shutdown");
    assert!(matches!(
        error,
        crate::supervisor::SupervisorControlCommandBodyError::Supervisor(_)
    ));
    assert!(supervisor.services().get("fresh").is_none());
}

fn response_json(line: &[u8]) -> serde_json::Value {
    assert_eq!(line.last(), Some(&b'\n'));
    serde_json::from_slice(&line[..line.len() - 1]).expect("response json")
}
