use crate::control::system::{
    ControlSecurityDescriptor, SystemAccess, SystemAccessDenied, SystemShutdownCommandRequest,
    admit_system_shutdown_command,
};
use crate::control::wire::{
    ControlFrameDecision, ControlRequestParseError, control_frame_decision, parse_control_request,
};
use crate::shutdown::{ShutdownFinalizationState, ShutdownKind};
use crate::supervisor::{SupervisorError, SupervisorSystemShutdownControlBodyError};

use super::super::{ScriptedClock, TestProcessController};
use super::SHUTDOWN_NS;
use super::fixture::{job_for, shutdown_fixture};

mod support;

use support::{
    CommandFinalizer, CommandFinalizerCall, FakeAccessCall, FakeSystemAccessChecker, control_peer,
};

#[test]
fn system_shutdown_command_enters_graceful_shutdown_from_clock_observation() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let parsed = parse_control_request(br#"{"command":"shutdown","type":"reboot"}"#)
        .expect("parsed shutdown");
    let request = admit_system_shutdown_command(&parsed, None).expect("admitted shutdown");

    let dispatch = supervisor
        .run_system_shutdown_command(request, &mut controller, &mut clock)
        .expect("system shutdown command");

    assert_eq!(dispatch.command.kind, ShutdownKind::Reboot);
    assert_eq!(dispatch.command.caller, None);
    assert_eq!(dispatch.shutdown.runtime.kind, ShutdownKind::Reboot);
    assert_eq!(dispatch.shutdown.runtime.initiated_at_ns, SHUTDOWN_NS);
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn authorized_shutdown_control_frame_enters_graceful_shutdown() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let ControlFrameDecision::Complete { body, consumed } =
        control_frame_decision(b"{\"command\":\"shutdown\",\"type\":\"halt\"}\n", 64)
    else {
        panic!("expected complete frame");
    };
    assert_eq!(
        consumed,
        b"{\"command\":\"shutdown\",\"type\":\"halt\"}\n".len()
    );

    let dispatch = supervisor
        .run_authorized_shutdown_control_body(&body, None, &mut controller, &mut clock)
        .expect("authorized shutdown body");

    assert_eq!(dispatch.command.kind, ShutdownKind::Halt);
    assert_eq!(dispatch.shutdown.runtime.kind, ShutdownKind::Halt);
    assert_eq!(dispatch.shutdown.runtime.initiated_at_ns, SHUTDOWN_NS);
}

#[test]
fn authorized_shutdown_control_body_rejects_non_shutdown_command() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);

    let error = supervisor
        .run_authorized_shutdown_control_body(
            br#"{"command":"list"}"#,
            None,
            &mut controller,
            &mut clock,
        )
        .expect_err("list is not a shutdown command");

    assert_eq!(
        error,
        SupervisorSystemShutdownControlBodyError::Admission(
            crate::control::system::SystemShutdownCommandAdmissionError::InvalidCommand,
        ),
    );
    assert!(supervisor.shutdown().is_none());
}

#[test]
fn checked_shutdown_control_body_authorizes_before_entering_shutdown() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let descriptor = ControlSecurityDescriptor::Default;
    let peer = control_peer();
    let caller = peer.summary.clone();
    let mut access = FakeSystemAccessChecker::allow(SystemAccess::ALL.bits());

    let dispatch = supervisor
        .run_checked_shutdown_control_body(
            br#"{"command":"shutdown","type":"reboot"}"#,
            &peer,
            &descriptor,
            &mut access,
            &mut controller,
            &mut clock,
        )
        .expect("checked shutdown");

    assert_eq!(dispatch.command.kind, ShutdownKind::Reboot);
    assert_eq!(dispatch.command.caller, Some(caller));
    assert_eq!(
        access.calls,
        vec![FakeAccessCall {
            token_fd: 44,
            descriptor,
            desired_access: SystemAccess::SHUTDOWN,
        }],
    );
    assert!(supervisor.shutdown().is_some());
}

#[test]
fn checked_shutdown_control_body_denies_without_mutation() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let descriptor = ControlSecurityDescriptor::Default;
    let peer = control_peer();
    let caller = peer.summary.clone();
    let mut access = FakeSystemAccessChecker::deny(0);

    let error = supervisor
        .run_checked_shutdown_control_body(
            br#"{"command":"shutdown","type":"poweroff"}"#,
            &peer,
            &descriptor,
            &mut access,
            &mut controller,
            &mut clock,
        )
        .expect_err("access denied");

    assert_eq!(
        error,
        SupervisorSystemShutdownControlBodyError::AccessDenied(Box::new(SystemAccessDenied {
            caller,
            desired_access: SystemAccess::SHUTDOWN,
            granted_access_bits: 0,
        })),
    );
    assert!(supervisor.shutdown().is_none());
    assert!(controller.signals.is_empty());
    assert!(controller.cgroup_kills.is_empty());
}

#[test]
fn checked_shutdown_control_body_rejects_parse_and_admission_before_access_check() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);
    let descriptor = ControlSecurityDescriptor::Default;
    let mut access = FakeSystemAccessChecker::allow(SystemAccess::ALL.bits());

    let parse_error = supervisor
        .run_checked_shutdown_control_body(
            b"{",
            &control_peer(),
            &descriptor,
            &mut access,
            &mut controller,
            &mut clock,
        )
        .expect_err("parse error");
    assert_eq!(
        parse_error,
        SupervisorSystemShutdownControlBodyError::Parse(ControlRequestParseError::MalformedRequest,),
    );

    let admission_error = supervisor
        .run_checked_shutdown_control_body(
            br#"{"command":"list"}"#,
            &control_peer(),
            &descriptor,
            &mut access,
            &mut controller,
            &mut clock,
        )
        .expect_err("admission error");
    assert_eq!(
        admission_error,
        SupervisorSystemShutdownControlBodyError::Admission(
            crate::control::system::SystemShutdownCommandAdmissionError::InvalidCommand,
        ),
    );
    assert!(access.calls.is_empty());
    assert!(supervisor.shutdown().is_none());
}

#[test]
fn system_shutdown_command_can_be_driven_to_final_action() {
    let mut supervisor = shutdown_fixture();
    let app_job = job_for(&supervisor, "app");
    let draining_job = job_for(&supervisor, "draining");
    let db_job = job_for(&supervisor, "db");
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS]);

    supervisor
        .run_system_shutdown_command(
            SystemShutdownCommandRequest {
                kind: ShutdownKind::Poweroff,
                caller: None,
            },
            &mut controller,
            &mut clock,
        )
        .expect("system shutdown command");
    supervisor
        .complete_shutdown_job(app_job, SHUTDOWN_NS + 1, 0, &mut controller)
        .expect("complete app");
    supervisor
        .complete_shutdown_job(draining_job, SHUTDOWN_NS + 2, 0, &mut controller)
        .expect("complete draining");
    supervisor
        .complete_shutdown_job(db_job, SHUTDOWN_NS + 3, 0, &mut controller)
        .expect("complete db");

    let mut finalizer = CommandFinalizer::default();
    let dispatch = supervisor
        .drive_shutdown(&mut controller, Some(&mut finalizer), SHUTDOWN_NS + 4)
        .expect("drive shutdown")
        .expect("finalization dispatch");

    assert!(dispatch.timeout.is_none());
    assert_eq!(
        dispatch.finalization.expect("finalization").finalization,
        ShutdownFinalizationState::Completed,
    );
    assert_eq!(
        finalizer.calls,
        vec![
            CommandFinalizerCall::SnapshotMounts,
            CommandFinalizerCall::RemountRootReadonly,
            CommandFinalizerCall::Sync,
            CommandFinalizerCall::Reboot(ShutdownKind::Poweroff),
        ],
    );
}

#[test]
fn system_shutdown_command_rejects_reentry() {
    let mut supervisor = shutdown_fixture();
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([SHUTDOWN_NS, SHUTDOWN_NS + 1]);

    supervisor
        .run_system_shutdown_command(
            SystemShutdownCommandRequest {
                kind: ShutdownKind::Poweroff,
                caller: None,
            },
            &mut controller,
            &mut clock,
        )
        .expect("first shutdown command");

    let error = supervisor
        .run_system_shutdown_command(
            SystemShutdownCommandRequest {
                kind: ShutdownKind::Reboot,
                caller: None,
            },
            &mut controller,
            &mut clock,
        )
        .expect_err("second shutdown command rejected");

    assert!(matches!(
        error,
        SupervisorError::Shutdown(crate::shutdown::ShutdownError::AlreadyInProgress {
            kind: ShutdownKind::Poweroff,
        })
    ));
}
