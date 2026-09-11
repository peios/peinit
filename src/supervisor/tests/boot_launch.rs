use crate::boundary::ProcessSetupStatus;
use crate::job::JobState;
use crate::operation::OperationState;
use crate::service::ServiceEnvironmentVariable;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::supervisor::{
    Supervisor, SupervisorProcessSetupDispatch, SupervisorServiceLaunchDispatch, SupervisorSettings,
};

use super::{
    APP_LAUNCH_NS, AUTHD_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

#[test]
fn boot_launches_dependency_then_dependent_and_projects_query_state() {
    let mut app = alive_service("app");
    app.requires.push("authd".to_string());
    let mut authd = alive_service("authd");
    authd.triggers.clear();

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, authd]);
    let mut clock = ScriptedClock::new([BOOT_NS, AUTHD_LAUNCH_NS, APP_LAUNCH_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    assert_eq!(
        boot.start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["authd"],
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9), process(4243, 10)]);
    let authd_launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch authd")
        .expect("authd launch dispatch");

    assert_eq!(authd_launch.launch.process.pid, 4242);
    assert_eq!(
        authd_launch
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    let app_operation = authd_launch.start_dispatches[0].ready.operation_id;
    let app_job = authd_launch.start_dispatches[0].job_id;
    assert_eq!(supervisor.pending_launch_jobs(), vec![app_job]);

    let authd_status = supervisor
        .service_status("authd")
        .expect("authd query status");
    assert_eq!(authd_status.state, ServiceState::Active);
    assert_eq!(authd_status.current_job.expect("authd job").pid, Some(4242),);
    assert!(authd_status.current_operation.is_none());

    let app_launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");

    assert_eq!(app_launch.launch.process.pid, 4243);
    assert!(app_launch.start_dispatches.is_empty());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(tokens.observed_jobs, vec!["authd", "app"]);
    assert_eq!(launcher.observed_jobs, vec!["authd", "app"]);
    assert_eq!(
        launcher.observed_notify_sockets,
        vec![
            Some(SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH.to_string()),
            Some(SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH.to_string()),
        ],
    );

    let app_status = supervisor.service_status("app").expect("app query status");
    assert_eq!(app_status.state, ServiceState::Active);
    assert_eq!(app_status.current_job.expect("app job").pid, Some(4243));
    assert!(app_status.current_operation.is_none());
    assert_eq!(
        supervisor
            .operation_status(app_operation)
            .expect("app operation")
            .state,
        OperationState::Completed,
    );
    assert_eq!(
        supervisor
            .list_services()
            .into_iter()
            .map(|item| (item.service, item.state))
            .collect::<Vec<_>>(),
        vec![
            ("app".to_string(), ServiceState::Active),
            ("authd".to_string(), ServiceState::Active),
        ],
    );
}

#[test]
fn service_launch_pending_setup_does_not_start_until_exec_success() {
    let app = alive_service("app");
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);

    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let job_id = supervisor.pending_launch_jobs()[0];
    let mut pending_process = process(4242, 9);
    pending_process.setup_status_fd = Some(55);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![pending_process]);

    let launch = supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("pending launch dispatch");
    assert!(matches!(
        launch,
        SupervisorServiceLaunchDispatch::PendingSetup(_)
    ));
    assert_eq!(supervisor.pending_process_setup_fds(), vec![55]);
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").state,
        JobState::Created
    );
    assert_eq!(
        supervisor
            .service_status("app")
            .expect("app query status")
            .state,
        ServiceState::Starting
    );

    let mut controller = TestProcessController::default();
    let completion = supervisor
        .process_pending_process_setup_status(
            55,
            ProcessSetupStatus::ExecSucceeded,
            APP_LAUNCH_NS + 1,
            &mut controller,
        )
        .expect("complete setup");
    let SupervisorProcessSetupDispatch::ServiceMainLaunched(dispatch) = completion else {
        panic!("expected service launch completion");
    };

    assert_eq!(dispatch.launch.process.pid, 4242);
    assert!(supervisor.pending_process_setup_fds().is_empty());
    assert_eq!(
        supervisor.jobs().get(job_id).expect("job").state,
        JobState::Running
    );
    assert_eq!(
        supervisor
            .service_status("app")
            .expect("app query status")
            .state,
        ServiceState::Active
    );
}

/// §8.1: `pid` and `pidfd` land on the record only once exec success is
/// confirmed by EOF on the error pipe. Until then the job is Created and both
/// stay null.
///
/// No guest can catch this window: it is the microseconds between fork and the
/// error pipe closing, and a submitter is not answered until the job has left
/// Created. Here the job is held in pending setup, its record inspected, and
/// then completed — the pid and pidfd appear only at that second step.
#[test]
fn pid_and_pidfd_land_only_on_exec_confirmation() {
    let app = alive_service("app");
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let job_id = supervisor.pending_launch_jobs()[0];

    let mut pending_process = process(4242, 9);
    pending_process.setup_status_fd = Some(55);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![pending_process]);
    supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("pending launch dispatch");

    // In the setup window: Created, and neither handle has landed, even though
    // the process was forked and its pid is known to the launcher.
    let held = supervisor.jobs().get(job_id).expect("job");
    assert_eq!(held.state, JobState::Created);
    assert_eq!(held.pid, None, "pid does not land until exec is confirmed");
    assert_eq!(held.pidfd, None, "nor the pidfd");

    // Exec confirmed by EOF on the error pipe: now both land and the job runs.
    let mut controller = TestProcessController::default();
    supervisor
        .process_pending_process_setup_status(
            55,
            ProcessSetupStatus::ExecSucceeded,
            APP_LAUNCH_NS + 1,
            &mut controller,
        )
        .expect("complete setup");
    let confirmed = supervisor.jobs().get(job_id).expect("job");
    assert_eq!(confirmed.state, JobState::Running);
    assert_eq!(confirmed.pid, Some(4242), "the pid lands on exec confirmation");
    assert_eq!(confirmed.pidfd, Some(9), "and so does the pidfd");
}

#[test]
fn boot_global_environment_is_layered_into_service_launch() {
    let mut app = alive_service("app");
    app.environment = vec![
        ServiceEnvironmentVariable {
            name: "PATH".to_string(),
            value: "/service/bin".to_string(),
        },
        ServiceEnvironmentVariable {
            name: "APP_MODE".to_string(),
            value: "service".to_string(),
        },
    ];

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services_with_global_environment(
        vec![app],
        vec![
            ServiceEnvironmentVariable {
                name: "PATH".to_string(),
                value: "/global/bin".to_string(),
            },
            ServiceEnvironmentVariable {
                name: "APP_MODE".to_string(),
                value: "global".to_string(),
            },
            ServiceEnvironmentVariable {
                name: "GLOBAL_ONLY".to_string(),
                value: "yes".to_string(),
            },
        ],
    );
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");

    assert_eq!(
        launcher.observed_paths,
        vec![Some("/service/bin".to_string())]
    );
    assert_eq!(
        launcher.observed_app_modes,
        vec![Some("service".to_string())],
    );
    assert_eq!(launcher.observed_global_only, vec![Some("yes".to_string())],);
}

#[test]
fn service_main_launch_uses_service_start_timeout_for_setup_handshake() {
    let mut app = alive_service("app");
    app.start_timeout_secs = 77;

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app]);
    let mut clock = ScriptedClock::new([BOOT_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch app")
        .expect("app launch dispatch");

    assert_eq!(launcher.observed_setup_timeouts, vec![77]);
}

#[test]
fn blocked_boot_service_is_projected_as_failed() {
    let mut app = alive_service("app");
    app.requires.push("disabled-db".to_string());
    let mut disabled = alive_service("disabled-db");
    disabled.disabled = true;

    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![app, disabled]);
    let mut clock = ScriptedClock::new([BOOT_NS]);
    let boot = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    assert!(boot.start_dispatches.is_empty());
    assert_eq!(boot.plan.blocked.len(), 1);
    assert_eq!(boot.plan.blocked[0].service, "app");

    let status = supervisor.service_status("app").expect("app query status");
    assert_eq!(status.state, ServiceState::Failed);
    assert_eq!(status.cause, Some(TransitionCause::DependencyFailure));
    assert!(status.current_operation.is_none());
    assert_eq!(
        supervisor
            .operation_status(boot.plan.blocked[0].operation_id)
            .expect("blocked boot operation")
            .state,
        OperationState::Failed,
    );
}
