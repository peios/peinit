use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use crate::boundary::{BoundaryError, ProcessLaunchError};
use crate::execution::notify::{NotifyAppliedField, NotifyApplyError};
use crate::fd_store::StoreFdOutcome;
use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::service::ServiceDefinition;
use crate::shutdown::ShutdownKind;
use crate::supervisor::{Supervisor, SupervisorError, SupervisorSettings};

use super::super::{
    APP_CRASH_NS, APP_LAUNCH_NS, BOOT_NS, RESTART_LAUNCH_NS, ScriptedClock, StaticRegistry,
    TestProcessController, TestProcessLauncher, TestTokenProvider, process, settings,
};
use super::{CONTROL_NS, NOTIFY_NS, active_app_supervisor, apply_notify};

#[test]
fn fdstore_notify_stores_attached_fd_with_name_and_poll_mode() {
    let mut supervisor = fd_store_app_supervisor(2);

    let dispatch = apply_notify(
        &mut supervisor,
        datagram_with_fds(
            8000,
            b"FDSTORE=1\nFDNAME=listener\nFDPOLL=0",
            vec![test_fd()],
        ),
        NOTIFY_NS,
    )
    .expect("apply fdstore notify");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![
            NotifyAppliedField::FdStore,
            NotifyAppliedField::FdName {
                name: "listener".to_string(),
            },
            NotifyAppliedField::FdPoll {
                value: "0".to_string(),
            },
        ],
    );
    let store = supervisor.fd_store().service("app").expect("app fd store");
    assert_eq!(store.len(), 1);
    assert_eq!(store.entries()[0].name, "listener");
    assert!(!store.entries()[0].poll);
    assert!(store.entries()[0].fd.duplicate().is_ok());
}

#[test]
fn fdstore_uses_default_name_and_capacity_limit() {
    let mut supervisor = fd_store_app_supervisor(1);
    let (first_fd, first_raw_fd) = test_fd_with_raw();
    let (second_fd, mut second_peer) = test_fd_with_peer();

    let dispatch = apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTORE=1", vec![first_fd, second_fd]),
        NOTIFY_NS,
    )
    .expect("apply fdstore notify");

    assert_eq!(
        dispatch.fd_store_rejections,
        vec![crate::supervisor::SupervisorFdStoreRejectionDispatch {
            service: "app".to_string(),
            name: "stored".to_string(),
            outcome: StoreFdOutcome::Full,
        }],
    );
    let store = supervisor.fd_store().service("app").expect("app fd store");
    assert_eq!(store.len(), 1);
    assert_eq!(store.entries()[0].name, "stored");
    assert!(store.entries()[0].poll);
    assert_eq!(store.entries()[0].fd.raw_fd(), first_raw_fd);
    assert!(peer_observes_closed(&mut second_peer));
}

#[test]
fn fdstore_disabled_drops_attached_fds_without_creating_store() {
    let mut supervisor = active_app_supervisor();
    let (fd, mut peer) = test_fd_with_peer();

    let dispatch = apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTORE=1\nFDNAME=listener", vec![fd]),
        NOTIFY_NS,
    )
    .expect("apply disabled fdstore notify");

    assert_eq!(
        dispatch.fd_store_rejections,
        vec![crate::supervisor::SupervisorFdStoreRejectionDispatch {
            service: "app".to_string(),
            name: "listener".to_string(),
            outcome: StoreFdOutcome::Disabled,
        }],
    );
    assert!(supervisor.fd_store().service("app").is_none());
    assert!(peer_observes_closed(&mut peer));
}

#[test]
fn fdstore_remove_deletes_all_matching_named_fds() {
    let mut supervisor = fd_store_app_supervisor(4);
    apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTORE=1\nFDNAME=listener", vec![test_fd()]),
        NOTIFY_NS,
    )
    .expect("store listener");
    apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTORE=1\nFDNAME=cache", vec![test_fd()]),
        NOTIFY_NS + 1,
    )
    .expect("store cache");
    apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTORE=1\nFDNAME=listener", vec![test_fd()]),
        NOTIFY_NS + 2,
    )
    .expect("store listener again");

    apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTOREREMOVE=1\nFDNAME=listener", Vec::new()),
        NOTIFY_NS + 3,
    )
    .expect("remove listener");

    let store = supervisor.fd_store().service("app").expect("app fd store");
    assert_eq!(store.len(), 1);
    assert_eq!(store.entries()[0].name, "cache");
}

/// PEI-836. §10.6: FDSTOREREMOVE=1 without FDNAME aborts the fd-store step
/// for that datagram, "so a datagram carrying both an unnamed remove and an
/// FDSTORE=1 performs neither, and the attached descriptors are dropped and
/// closed". The store used to fill in its default name before the remove
/// guard looked, so the pairing removed the default-named entries *and*
/// stored the attached descriptor under the same name.
#[test]
fn an_unnamed_remove_beside_a_store_performs_neither() {
    let mut supervisor = fd_store_app_supervisor(4);
    apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTORE=1", vec![test_fd()]),
        NOTIFY_NS,
    )
    .expect("store under the default name");
    let (attached, mut peer) = test_fd_with_peer();

    let dispatch = apply_notify(
        &mut supervisor,
        datagram_with_fds(8000, b"FDSTORE=1\nFDSTOREREMOVE=1", vec![attached]),
        NOTIFY_NS + 1,
    )
    .expect("apply unnamed remove beside a store");

    assert!(dispatch.fd_store_rejections.is_empty());
    let store = supervisor.fd_store().service("app").expect("app fd store");
    assert_eq!(store.len(), 1, "the unnamed remove was performed");
    assert_eq!(store.entries()[0].name, "stored");
    assert!(
        store.entries()[0].fd.duplicate().is_ok(),
        "the previously stored descriptor was replaced",
    );
    assert!(
        peer_observes_closed(&mut peer),
        "the attached descriptor was stored rather than dropped",
    );
}

#[test]
fn unauthenticated_fdstore_notify_is_rejected_without_storing_fd() {
    let mut supervisor = fd_store_app_supervisor(2);
    let (fd, mut peer) = test_fd_with_peer();

    let error = apply_notify(
        &mut supervisor,
        datagram_with_fds(9999, b"FDSTORE=1\nFDNAME=listener", vec![fd]),
        NOTIFY_NS,
    )
    .expect_err("reject unauthenticated fdstore");

    assert_eq!(
        error,
        SupervisorError::Notify(NotifyApplyError::UnauthenticatedSender { pid: 9999 }),
    );
    assert!(supervisor.fd_store().service("app").is_none());
    assert!(peer_observes_closed(&mut peer));
}

#[test]
fn stored_fds_are_inherited_on_restart_and_cleared_after_successful_launch() {
    let mut supervisor = fd_store_app_supervisor(4);
    store_fd(&mut supervisor, "listener", NOTIFY_NS);
    store_fd(&mut supervisor, "cache", NOTIFY_NS + 1);

    queue_restart(&mut supervisor);

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8001, 51)]);
    let mut clock = ScriptedClock::new([RESTART_LAUNCH_NS]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch restart")
        .expect("restart launch dispatch");

    assert_eq!(launcher.observed_listen_fds, vec![Some("2".to_string())]);
    assert_eq!(
        launcher.observed_listen_fdnames,
        vec![Some("listener:cache".to_string())],
    );
    assert_eq!(
        launcher.observed_inherited_fd_names,
        vec![vec!["listener".to_string(), "cache".to_string()]],
    );
    assert!(supervisor.fd_store().service("app").is_none());
}

#[test]
fn stored_fds_are_retained_when_restart_launch_fails() {
    let mut supervisor = fd_store_app_supervisor(2);
    store_fd(&mut supervisor, "listener", NOTIFY_NS);

    queue_restart(&mut supervisor);

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::results(vec![Err(BoundaryError::ProcessLaunch(
        ProcessLaunchError::parent_setup("scripted launch failure"),
    ))]);
    let mut clock = ScriptedClock::new([RESTART_LAUNCH_NS]);
    supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("apply launch failure")
        .expect("launch failure dispatch");

    let store = supervisor.fd_store().service("app").expect("app fd store");
    assert_eq!(store.len(), 1);
    assert_eq!(store.entries()[0].name, "listener");
    assert_eq!(
        launcher.observed_inherited_fd_names,
        vec![vec!["listener".to_string()]],
    );
}

#[test]
fn stored_fds_are_cleared_after_explicit_stop_completes() {
    let mut supervisor = fd_store_app_supervisor(2);
    store_fd(&mut supervisor, "listener", NOTIFY_NS);
    let job_id = current_app_job(&supervisor);
    let mut controller = TestProcessController::default();

    let mut command_clock = ScriptedClock::new([CONTROL_NS - 1]);
    supervisor
        .stop_service("app", None, &mut command_clock)
        .expect("request stop");
    let mut control_clock = ScriptedClock::new([CONTROL_NS]);
    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut control_clock)
        .expect("execute stop")
        .expect("stop execution");

    assert!(supervisor.fd_store().service("app").is_some());
    supervisor
        .complete_job(job_id, CONTROL_NS + 1, 0)
        .expect("complete stopped app");

    assert!(supervisor.fd_store().service("app").is_none());
}

#[test]
fn stored_fds_are_cleared_after_shutdown_stop_completes() {
    let mut supervisor = fd_store_app_supervisor(2);
    store_fd(&mut supervisor, "listener", NOTIFY_NS);
    let job_id = current_app_job(&supervisor);
    let mut controller = TestProcessController::default();

    supervisor
        .begin_shutdown(ShutdownKind::Poweroff, &mut controller, CONTROL_NS)
        .expect("begin shutdown");
    assert!(supervisor.fd_store().service("app").is_some());
    supervisor
        .complete_shutdown_job(job_id, CONTROL_NS + 1, 0, &mut controller)
        .expect("complete shutdown stop");

    assert!(supervisor.fd_store().service("app").is_none());
}

fn fd_store_app_supervisor(fd_store_max: u32) -> Supervisor {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.readiness = crate::service::Readiness::Alive;
    app.fd_store_max = fd_store_max;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot app");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(8000, 50)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    supervisor
}

fn store_fd(supervisor: &mut Supervisor, name: &str, observed_at_ns: u64) {
    apply_notify(
        supervisor,
        datagram_with_fds(
            8000,
            format!("FDSTORE=1\nFDNAME={name}").as_bytes(),
            vec![test_fd()],
        ),
        observed_at_ns,
    )
    .expect("store fd");
}

fn queue_restart(supervisor: &mut Supervisor) {
    let job_id = supervisor
        .service_status("app")
        .expect("active app")
        .current_job
        .expect("app job")
        .id;
    supervisor
        .complete_job(job_id, APP_CRASH_NS, 1)
        .expect("crash app");
    let deadline = supervisor
        .next_restart_backoff_deadline()
        .expect("restart deadline");
    supervisor
        .process_due_restart_backoffs(deadline.due_at_ns)
        .expect("due restart");
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
}

fn current_app_job(supervisor: &Supervisor) -> crate::ids::JobId {
    supervisor
        .service_status("app")
        .expect("active app")
        .current_job
        .expect("app job")
        .id
}

fn datagram_with_fds(pid: u32, payload: &[u8], fds: Vec<OwnedFd>) -> NotifyDatagram {
    NotifyDatagram {
        payload: payload.to_vec(),
        credentials: NotifyCredentials {
            pid,
            uid: 0,
            gid: 0,
        },
        fds,
    }
}

fn test_fd() -> OwnedFd {
    let (left, _right) = UnixStream::pair().expect("socket pair");
    left.into()
}

fn test_fd_with_raw() -> (OwnedFd, i32) {
    let fd = test_fd();
    let raw_fd = fd.as_raw_fd();
    (fd, raw_fd)
}

fn test_fd_with_peer() -> (OwnedFd, UnixStream) {
    let (left, right) = UnixStream::pair().expect("socket pair");
    right.set_nonblocking(true).expect("set peer nonblocking");
    (left.into(), right)
}

fn peer_observes_closed(peer: &mut UnixStream) -> bool {
    let mut buf = [0_u8; 1];
    match peer.read(&mut buf) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("read peer: {error}"),
    }
}

// PEI-346. §3.5: a service whose definition has been withdrawn is not killed,
// but "when that instance exits it is NOT restarted (RestartPolicy is moot --
// the definition is gone), and peinit then discards the entry entirely".
//
// The clean-exit path did that. A restart-eligible crash did not: it went to
// Backoff, which is a state the entry could never leave. Two halves of the
// deadlock, in different files — `restart_backoff_deadlines` skips
// definition-removed entries so nothing moved it out, and Backoff keeps an
// entry alive after removal so nothing discarded it. The entry stayed in
// `status` describing a restart that would never happen, refused every
// lifecycle command with UNKNOWN_SERVICE, and held its stored descriptors open
// in PID 1 for the life of the process.
#[test]
fn a_definition_removed_service_that_crashes_is_discarded_with_its_fd_store() {
    let mut supervisor = fd_store_app_supervisor(2);
    store_fd(&mut supervisor, "listener", NOTIFY_NS);
    assert_eq!(
        supervisor
            .fd_store()
            .service("app")
            .expect("app fd store")
            .len(),
        1,
    );

    // The registry no longer defines it. Active retains the entry until the
    // instance drains, which is the window this is about.
    supervisor
        .services
        .apply_definition_snapshot(Vec::new())
        .expect("withdraw the definition");
    assert!(
        supervisor
            .service_status("app")
            .expect("app")
            .definition_removed,
    );
    let job_id = supervisor
        .service_status("app")
        .expect("app")
        .current_job
        .expect("app job")
        .id;

    supervisor
        .complete_job(job_id, APP_CRASH_NS, 1)
        .expect("app crashes");

    assert!(
        supervisor.service_status("app").is_err(),
        "a definition-removed service that crashed is still in the table",
    );
    assert!(
        supervisor.fd_store().service("app").is_none(),
        "the discarded service's descriptors are still open in PID 1",
    );
    assert!(
        supervisor.next_restart_backoff_deadline().is_none(),
        "a restart was scheduled for a service with no definition to restart",
    );
}
