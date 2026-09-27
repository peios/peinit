//! `TTYPath` exclusivity and the `tty:released` hand-off, end to end.
//!
//! The case that drove it: an installed system's first boot runs `oobe` on
//! `/dev/console`, and `login-console` wants the same device. Before this,
//! both started and interleaved — a login prompt through the middle of a
//! setup form, with the password echoed into whatever redrew last.

use crate::ids::JobId;
use crate::service::runtime::{ServiceState, TransitionCause};
use crate::service::{ServiceDefinition, ServiceTrigger};
use crate::supervisor::{Supervisor, SupervisorSettings};

use super::{
    ADMIN_START_NS, BOOT_NS, DB_LAUNCH_NS, ON_DEMAND_APP_LAUNCH_NS, ScriptedClock, StaticRegistry,
    TestProcessLauncher, TestTokenProvider, alive_service, process, settings,
};

const CONSOLE: &str = "/dev/console";

/// A console service that waits its turn: demand-only, so a test drives its
/// start explicitly, plus the trigger that makes the release reach it.
fn console_service(name: &str, precedence: u32) -> ServiceDefinition {
    let mut definition = alive_service(name);
    definition.triggers = vec![ServiceTrigger::TtyReleased];
    definition.console_path = Some(CONSOLE.to_string());
    definition.console_precedence = precedence;
    definition
}

fn launch(supervisor: &mut Supervisor, at_ns: u64, pid: u32, pidfd: i32) {
    let mut clock = ScriptedClock::new([at_ns]);
    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(pid, pidfd)]);
    supervisor
        .launch_next_pending_job(&mut tokens, &mut launcher, &mut clock)
        .expect("launch pending service")
        .expect("launch dispatch");
}

fn current_job(supervisor: &Supervisor, service: &str) -> JobId {
    supervisor
        .service_status(service)
        .expect("service status")
        .current_job
        .expect("current job")
        .id
}

fn state(supervisor: &Supervisor, service: &str) -> ServiceState {
    supervisor.service_status(service).expect("status").state
}

/// The whole feature in one boot: the console is taken, the second claimant is
/// skipped rather than left to scribble over it, and it gets the device when
/// the holder is done.
#[test]
fn a_busy_console_skips_the_second_claimant_and_hands_over_when_it_frees() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![
        console_service("oobe", 100),
        console_service("login-console", 0),
    ]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        ADMIN_START_NS,
        DB_LAUNCH_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    supervisor
        .start_service("oobe", None, &mut clock)
        .expect("start oobe");
    launch(&mut supervisor, DB_LAUNCH_NS, 9300, 130);
    assert_eq!(state(&supervisor, "oobe"), ServiceState::Active);

    // The console is somebody else's, so this start ends before it begins.
    supervisor
        .start_service("login-console", None, &mut clock)
        .expect("start login-console");
    assert_eq!(state(&supervisor, "login-console"), ServiceState::Skipped);
    assert_eq!(
        supervisor
            .service_status("login-console")
            .expect("status")
            .cause,
        Some(TransitionCause::TtyUnavailable),
    );
    assert!(
        supervisor.pending_launch_jobs().is_empty(),
        "a skipped service must not have queued a process",
    );

    let oobe_job = current_job(&supervisor, "oobe");
    let released = supervisor
        .complete_job(oobe_job, ON_DEMAND_APP_LAUNCH_NS, 0)
        .expect("oobe exits");

    assert_eq!(
        released
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["login-console"],
        "the console coming free must start the service queued on it",
    );
    assert_eq!(state(&supervisor, "login-console"), ServiceState::Starting);
}

/// The release must not depend on the holder having succeeded. A first-boot
/// setup flow that crashes has still let go of the console, and the login
/// prompt is wanted *more* in that case, not less.
#[test]
fn a_holder_that_fails_still_releases_its_console() {
    let mut oobe = console_service("oobe", 100);
    oobe.restart_policy = crate::service::RestartPolicy::Never;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![oobe, console_service("login-console", 0)]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        ADMIN_START_NS,
        DB_LAUNCH_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .start_service("oobe", None, &mut clock)
        .expect("start oobe");
    launch(&mut supervisor, DB_LAUNCH_NS, 9310, 131);

    let oobe_job = current_job(&supervisor, "oobe");
    let released = supervisor
        .complete_job(oobe_job, ON_DEMAND_APP_LAUNCH_NS, 1)
        .expect("oobe crashes");

    assert_eq!(state(&supervisor, "oobe"), ServiceState::Failed);
    assert_eq!(
        released
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["login-console"],
    );
}

/// Only services that asked for it. A console service with no `tty:released`
/// trigger stays where it is — the queue is opt-in, and starting a service
/// nobody asked to start is worse than leaving a terminal idle.
#[test]
fn a_console_service_without_the_trigger_is_not_woken() {
    let mut login = console_service("login-console", 0);
    login.triggers.clear();
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![console_service("oobe", 100), login]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        ADMIN_START_NS,
        DB_LAUNCH_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .start_service("oobe", None, &mut clock)
        .expect("start oobe");
    launch(&mut supervisor, DB_LAUNCH_NS, 9320, 132);

    let oobe_job = current_job(&supervisor, "oobe");
    let released = supervisor
        .complete_job(oobe_job, ON_DEMAND_APP_LAUNCH_NS, 0)
        .expect("oobe exits");

    assert!(released.start_dispatches.is_empty());
    assert_eq!(state(&supervisor, "login-console"), ServiceState::Inactive);
}

/// Precedence decides who is offered a freed console, not who asked first.
#[test]
fn the_highest_precedence_waiter_is_offered_the_freed_console() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![
        console_service("holder", 200),
        // Named so that the lower-precedence service sorts first: if
        // precedence were ignored, "aaa-low" is what would be started.
        console_service("aaa-low", 1),
        console_service("zzz-high", 50),
    ]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        ADMIN_START_NS,
        DB_LAUNCH_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .start_service("holder", None, &mut clock)
        .expect("start holder");
    launch(&mut supervisor, DB_LAUNCH_NS, 9330, 133);

    let holder_job = current_job(&supervisor, "holder");
    let released = supervisor
        .complete_job(holder_job, ON_DEMAND_APP_LAUNCH_NS, 0)
        .expect("holder exits");

    assert_eq!(
        released
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["zzz-high"],
    );
    assert_eq!(state(&supervisor, "aaa-low"), ServiceState::Inactive);
}

/// A different device is a different queue. Two consoles must not arbitrate
/// against each other, or a machine with several terminals runs one at a time.
#[test]
fn two_terminals_are_two_queues() {
    let mut second = console_service("tty2-login", 0);
    second.console_path = Some("/dev/tty2".to_string());
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![console_service("console-login", 0), second]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        ADMIN_START_NS,
        DB_LAUNCH_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");

    supervisor
        .start_service("console-login", None, &mut clock)
        .expect("start console-login");
    launch(&mut supervisor, DB_LAUNCH_NS, 9340, 134);
    supervisor
        .start_service("tty2-login", None, &mut clock)
        .expect("start tty2-login");

    assert_eq!(state(&supervisor, "console-login"), ServiceState::Active);
    assert_eq!(state(&supervisor, "tty2-login"), ServiceState::Starting);
}

/// The bug jack hit on a real first boot: setup said "you can log in now"
/// and no prompt appeared until the machine was restarted.
///
/// Setup retires itself by removing its own two service definitions and
/// then exiting, both within a second. peinit discards a removed
/// definition's entry the moment the service reaches a state that does
/// not retain one — which is the same moment it releases its terminal —
/// so the reaction, which looked the `TTYPath` up in the table after the
/// transition, found nothing and handed the console to nobody.
///
/// The terminal now rides on the transition, which is the only place both
/// the definition and the two states are in hand at once.
#[test]
fn a_holder_that_deleted_its_own_definition_still_hands_the_console_on() {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let oobe = console_service("oobe-tui", 100);
    let login = console_service("login-console", 0);
    let mut registry = StaticRegistry::services(vec![oobe, login.clone()]);
    let mut clock = ScriptedClock::new([
        BOOT_NS,
        ADMIN_START_NS,
        DB_LAUNCH_NS,
        ON_DEMAND_APP_LAUNCH_NS,
    ]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
        .start_service("oobe-tui", None, &mut clock)
        .expect("start oobe-tui");
    launch(&mut supervisor, DB_LAUNCH_NS, 9350, 135);
    assert_eq!(state(&supervisor, "oobe-tui"), ServiceState::Active);

    // Setup finishes: it deletes its own key from the registry, peinit
    // notices, and only then does the process exit.
    let mut retired = StaticRegistry::services(vec![login]);
    supervisor
        .reload_config_from_registry(&mut retired)
        .expect("reload after retirement");
    assert!(
        supervisor
            .service_status("oobe-tui")
            .expect("status")
            .definition_removed,
        "the definition is gone but the service is still running",
    );

    let oobe_job = current_job(&supervisor, "oobe-tui");
    let released = supervisor
        .complete_job(oobe_job, ON_DEMAND_APP_LAUNCH_NS, 0)
        .expect("oobe-tui exits");

    assert_eq!(
        released
            .start_dispatches
            .iter()
            .map(|dispatch| dispatch.ready.service.as_str())
            .collect::<Vec<_>>(),
        vec!["login-console"],
        "a console whose owner retired itself must still reach the prompt",
    );
    assert_eq!(state(&supervisor, "login-console"), ServiceState::Starting);
}

/// PEI-808. Two boot-triggered services naming one terminal. The boot plan
/// admits both, and its pre-start check ran for both before either had moved,
/// so both passed it; the loser was exec'd and died in `TIOCSCTTY`, went to
/// Backoff, and its relaunch's correct skip was an `InvalidTransition` that
/// ended the runtime loop. Now the terminal is asked about again at the
/// moment of the transition, and the loser is skipped exactly as a demand
/// start would be — in plan order, not precedence order (§11.6).
#[test]
fn two_boot_plan_services_naming_one_terminal_start_one_and_skip_the_other() {
    let mut first = alive_service("pt-aaa");
    first.console_path = Some("/dev/tty2".to_string());
    let mut second = alive_service("pt-zzz");
    second.console_path = Some("/dev/tty2".to_string());
    second.console_precedence = 99;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![first, second]);
    let mut clock = ScriptedClock::new([BOOT_NS]);

    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("a boot with two claimants on one terminal is not an error");

    assert_eq!(state(&supervisor, "pt-aaa"), ServiceState::Starting);
    assert_eq!(state(&supervisor, "pt-zzz"), ServiceState::Skipped);
    assert_eq!(
        supervisor.service_status("pt-zzz").expect("status").cause,
        Some(TransitionCause::TtyUnavailable),
    );
    assert_eq!(
        supervisor.pending_launch_jobs().len(),
        1,
        "only the winner has a process to launch",
    );
    // The loser's operation completed as a skip, so nothing waits on it.
    let skipped = supervisor
        .service_status("pt-zzz")
        .expect("status")
        .current_operation;
    assert!(skipped.is_none(), "{skipped:?}");
}
