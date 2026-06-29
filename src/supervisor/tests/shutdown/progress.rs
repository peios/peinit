use crate::boundary::ProcessSignal;
use crate::service::runtime::ServiceState;
use crate::shutdown::{ShutdownFinalizationState, ShutdownKind};

use super::super::TestProcessController;
use super::SHUTDOWN_NS;
use super::fixture::{job_for, shutdown_fixture};

#[test]
fn shutdown_terminal_events_advance_reverse_dependency_waves() {
    let mut supervisor = shutdown_fixture();
    let app_job = job_for(&supervisor, "app");
    let draining_job = job_for(&supervisor, "draining");
    let db_job = job_for(&supervisor, "db");
    let mut controller = TestProcessController::default();
    supervisor
        .begin_shutdown(ShutdownKind::Reboot, &mut controller, SHUTDOWN_NS)
        .expect("begin shutdown");

    let app_dispatch = supervisor
        .complete_shutdown_job(app_job, SHUTDOWN_NS + 1, 0, &mut controller)
        .expect("complete app");

    assert!(app_dispatch.next_wave.is_empty());
    assert_eq!(
        app_dispatch.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").state,
        ServiceState::Inactive,
    );

    let draining_dispatch = supervisor
        .complete_shutdown_job(draining_job, SHUTDOWN_NS + 2, 0, &mut controller)
        .expect("complete draining");

    assert_eq!(draining_dispatch.next_wave.len(), 1);
    assert_eq!(draining_dispatch.next_wave[0].service, "db");
    assert_eq!(
        draining_dispatch.next_wave[0].signal,
        Some(ProcessSignal::Sigterm),
    );
    assert_eq!(
        draining_dispatch.finalization,
        ShutdownFinalizationState::WaitingForServices,
    );
    assert_eq!(
        supervisor.service_status("db").expect("db").state,
        ServiceState::Stopping,
    );

    let db_dispatch = supervisor
        .complete_shutdown_job(db_job, SHUTDOWN_NS + 3, 0, &mut controller)
        .expect("complete db");

    assert!(db_dispatch.next_wave.is_empty());
    assert_eq!(db_dispatch.finalization, ShutdownFinalizationState::Ready);
    assert_eq!(
        supervisor.shutdown().expect("shutdown").finalization,
        ShutdownFinalizationState::Ready,
    );
}
