use crate::service::runtime::ServiceState;
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, MIGRATE_DONE_NS, MIGRATE_LAUNCH_NS, ScriptedClock, StaticRegistry,
    TestProcessLauncher, TestTokenProvider, alive_service, oneshot_service, process, settings,
};

#[test]
fn oneshot_terminal_success_releases_dependent_start() {
    let mut app = alive_service("app");
    app.requires.push("migrate".to_string());
    let mut migrate = oneshot_service("migrate");
    migrate.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, migrate]);
    let mut clock = ScriptedClock::new([BOOT_NS, MIGRATE_LAUNCH_NS, APP_LAUNCH_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    assert_eq!(boot.start_dispatches[0].ready.service, "migrate");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(6000, 30), process(6001, 31)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch migrate")
        .expect("migrate launch dispatch");
    let migrate_job = supervisor
        .service_status("migrate")
        .expect("migrate status")
        .current_job
        .expect("migrate job")
        .id;
    assert!(supervisor.pending_launch_jobs().is_empty());

    let terminal = supervisor
        .complete_job(migrate_job, MIGRATE_DONE_NS, 0)
        .expect("complete migrate");
    assert_eq!(
        terminal
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    assert_eq!(
        supervisor
            .service_status("migrate")
            .expect("migrate released")
            .state,
        ServiceState::Inactive,
    );

    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");
    assert_eq!(
        supervisor.service_status("app").expect("app active").state,
        ServiceState::Active,
    );
}
