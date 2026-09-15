use crate::notify::{NotifyCredentials, NotifyDatagram};
use crate::service::runtime::ServiceState;
use crate::service::{ErrorControl, Readiness, ServiceDefinition, ServiceSecurityDescriptor};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessController,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

#[test]
fn phase1_registryd_activation_is_retained_through_phase2_boot() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));

    let boot = supervisor
        .prepare_phase1_registryd_start(BOOT_NS)
        .expect("prepare Phase 1 registryd start");
    assert_eq!(
        boot.plan
            .starts
            .iter()
            .map(|start| start.service.as_str())
            .collect::<Vec<_>>(),
        vec![ServiceDefinition::REGISTRYD_NAME],
    );
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);

    let definition = supervisor
        .services()
        .definition(ServiceDefinition::REGISTRYD_NAME)
        .expect("registryd definition");
    assert_eq!(
        definition.image_path,
        ServiceDefinition::REGISTRYD_IMAGE_PATH
    );
    assert_eq!(definition.identity, "SYSTEM");
    assert_eq!(definition.readiness, Readiness::Notify);
    assert_eq!(definition.error_control, ErrorControl::Critical);
    assert_eq!(
        supervisor
            .service_status(ServiceDefinition::REGISTRYD_NAME)
            .expect("registryd status")
            .state,
        ServiceState::Starting,
    );

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(7000, 70)]);
    let mut clock = ScriptedClock::new([BOOT_NS + 1_000]);
    let launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch registryd")
        .expect("registryd launch dispatch");

    assert_eq!(
        tokens.observed_jobs,
        vec![ServiceDefinition::REGISTRYD_NAME]
    );
    assert_eq!(
        launcher.observed_jobs,
        vec![ServiceDefinition::REGISTRYD_NAME]
    );
    assert_eq!(
        launcher.observed_notify_sockets,
        vec![Some(
            SupervisorSettings::DEFAULT_NOTIFY_SOCKET_PATH.to_string()
        )],
    );
    supervisor.retain_service_launch_for_runtime(launch.launch.clone());

    let mut controller = TestProcessController::default();
    supervisor
        .apply_notify_datagram(datagram(7000, b"READY=1"), BOOT_NS + 2_000, &mut controller)
        .expect("apply registryd ready notification")
        .into_service()
        .expect("service notify outcome");
    assert_eq!(
        supervisor
            .service_status(ServiceDefinition::REGISTRYD_NAME)
            .expect("registryd status")
            .state,
        ServiceState::Active,
    );
    assert_eq!(supervisor.retained_service_launches().len(), 1);

    let mut registryd = ServiceDefinition::compiled_in_registryd();
    registryd.image_path = "/registry/registryd".to_string();
    let mut app = alive_service("app");
    app.requires
        .push(ServiceDefinition::REGISTRYD_NAME.to_string());
    let mut registry = StaticRegistry::services(vec![registryd, app]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);

    let phase2 = supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("run Phase 2 boot with retained registryd");

    assert_eq!(
        phase2
            .plan
            .starts
            .iter()
            .map(|start| start.service.as_str())
            .collect::<Vec<_>>(),
        vec!["app"],
    );
    let registryd_entry = supervisor
        .services()
        .get(ServiceDefinition::REGISTRYD_NAME)
        .expect("merged registryd entry");
    assert_eq!(
        registryd_entry.definition.image_path,
        ServiceDefinition::REGISTRYD_IMAGE_PATH,
    );
    assert_eq!(
        registryd_entry
            .pending_definition
            .as_ref()
            .expect("pending registryd definition")
            .image_path,
        "/registry/registryd",
    );
    let status = supervisor
        .service_status(ServiceDefinition::REGISTRYD_NAME)
        .expect("registryd status after Phase 2");
    assert_eq!(status.state, ServiceState::Active);
    assert_eq!(status.current_job.expect("registryd job").pid, Some(7000));
    assert_eq!(supervisor.pending_launch_jobs().len(), 1);
}

/// TRM §4.6: a service whose definition carries no `ServiceSecurity` takes
/// the one on `Machine\System\Services`. registryd has no definition in the
/// registry at all, and used to keep its compiled-in default whatever the
/// Services key said — so narrowing that key reached every service except
/// the one Critical service whose stop takes the machine down (PEI-1072).
#[test]
fn registryd_inherits_the_services_key_descriptor_once_phase2_has_read_it() {
    let inherited = ServiceSecurityDescriptor::RegistryBinary(vec![1, 2, 3]);
    let mut supervisor = active_registryd_after_phase1();
    let mut registry = StaticRegistry::services(vec![alive_service("app")])
        .with_inherited_service_security(inherited.clone());
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);

    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("run Phase 2 boot with retained registryd");

    let registryd = supervisor
        .services()
        .get(ServiceDefinition::REGISTRYD_NAME)
        .expect("retained registryd entry");
    assert_eq!(registryd.definition.service_security, inherited);
    assert!(registryd.definition.compiled_in);
    assert!(registryd.pending_definition.is_none());
    assert_eq!(registryd.runtime.state, ServiceState::Active);
}

/// TRM §2.3: a registry definition of registryd is merged onto the retained
/// activation, so a `ServiceSecurity` it sets is registryd's own and wins
/// over the inherited one, exactly as for any other service.
#[test]
fn a_registry_definition_of_registryd_sets_its_own_descriptor() {
    let own = ServiceSecurityDescriptor::RegistryBinary(vec![9]);
    let mut supervisor = active_registryd_after_phase1();
    let mut registryd = ServiceDefinition::compiled_in_registryd();
    registryd.compiled_in = false;
    registryd.service_security = own.clone();
    let mut registry = StaticRegistry::services(vec![registryd, alive_service("app")])
        .with_inherited_service_security(ServiceSecurityDescriptor::RegistryBinary(vec![1, 2, 3]));
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);

    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("run Phase 2 boot with a seeded registryd");

    let registryd = supervisor
        .services()
        .get(ServiceDefinition::REGISTRYD_NAME)
        .expect("merged registryd entry");
    assert_eq!(registryd.definition.service_security, own);
    // Provenance is peinit's: the registry entry does not make the daemon
    // that serves the registry removable on the next reload.
    assert!(registryd.definition.compiled_in);
    assert_eq!(registryd.runtime.state, ServiceState::Active);
}

/// A reload that changes the Services-key descriptor reaches registryd like
/// any other service without a descriptor of its own, and removing the key
/// value drops it back to the built-in default.
#[test]
fn reload_config_reaches_registryd_with_the_services_key_descriptor() {
    let mut supervisor = active_registryd_after_phase1();
    let mut registry = StaticRegistry::services(vec![alive_service("app")]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("run Phase 2 boot with retained registryd");
    let narrowed = ServiceSecurityDescriptor::RegistryBinary(vec![4, 4]);

    let outcome = supervisor
        .reload_config_from_registry(
            &mut StaticRegistry::services(vec![alive_service("app")])
                .with_inherited_service_security(narrowed.clone()),
        )
        .expect("reload with a narrowed Services key");

    assert_eq!(
        outcome.summary.updated,
        vec![ServiceDefinition::REGISTRYD_NAME.to_string()]
    );
    let registryd = supervisor
        .services()
        .get(ServiceDefinition::REGISTRYD_NAME)
        .expect("registryd entry");
    assert_eq!(registryd.definition.service_security, narrowed);
    assert!(registryd.definition.compiled_in);
    assert!(!registryd.definition_removed);

    let outcome = supervisor
        .reload_config_from_registry(&mut StaticRegistry::services(vec![alive_service("app")]))
        .expect("reload with the Services key value removed");

    assert_eq!(
        outcome.summary.updated,
        vec![ServiceDefinition::REGISTRYD_NAME.to_string()]
    );
    assert_eq!(
        supervisor
            .services()
            .definition(ServiceDefinition::REGISTRYD_NAME)
            .expect("registryd definition")
            .service_security,
        ServiceSecurityDescriptor::Default,
    );
}

/// A supervisor after Phase 1: registryd launched, READY, and Active.
fn active_registryd_after_phase1() -> Supervisor {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    supervisor
        .prepare_phase1_registryd_start(BOOT_NS)
        .expect("prepare Phase 1 registryd start");
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(7000, 70)]);
    let mut clock = ScriptedClock::new([BOOT_NS + 1_000]);
    let launch = supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch registryd")
        .expect("registryd launch dispatch");
    supervisor.retain_service_launch_for_runtime(launch.launch.clone());
    let mut controller = TestProcessController::default();
    supervisor
        .apply_notify_datagram(datagram(7000, b"READY=1"), BOOT_NS + 2_000, &mut controller)
        .expect("apply registryd ready notification")
        .into_service()
        .expect("service notify outcome");
    assert_eq!(
        supervisor
            .service_status(ServiceDefinition::REGISTRYD_NAME)
            .expect("registryd status")
            .state,
        ServiceState::Active,
    );
    supervisor
}

fn datagram(pid: u32, payload: &[u8]) -> NotifyDatagram {
    NotifyDatagram {
        payload: payload.to_vec(),
        credentials: NotifyCredentials {
            pid,
            uid: 0,
            gid: 0,
        },
        fds: Vec::new(),
    }
}
