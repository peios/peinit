use super::*;
use crate::service::ServiceTrigger;
use crate::service::runtime::{ServiceTransition, TransitionCause};

const CONSOLE: &str = "/dev/console";

fn table(definitions: Vec<ServiceDefinition>) -> ServiceTable {
    ServiceTable::from_boot_snapshot(definitions).expect("service table")
}

fn terminal_service(name: &str, precedence: u32) -> ServiceDefinition {
    let mut definition = ServiceDefinition::simple_system_boot(name, "/bin/login");
    definition.triggers = vec![ServiceTrigger::TtyReleased];
    definition.console_path = Some(CONSOLE.to_string());
    definition.console_precedence = precedence;
    definition
}

/// Walk a service to `state` through legal transitions — fabricating states
/// would let these tests assert against combinations peinit cannot produce.
fn set_state(services: &mut ServiceTable, service: &str, state: ServiceState) {
    let step = |services: &mut ServiceTable, to, cause| {
        services
            .transition_service(service, ServiceTransition { to, cause })
            .expect("legal transition");
    };
    if state == ServiceState::Inactive {
        return;
    }
    if state == ServiceState::Skipped {
        step(services, state, TransitionCause::TtyUnavailable);
        return;
    }
    step(
        services,
        ServiceState::Starting,
        TransitionCause::ExplicitStart,
    );
    match state {
        ServiceState::Starting => {}
        ServiceState::Active | ServiceState::Completed => {
            step(services, state, TransitionCause::ExplicitStart)
        }
        ServiceState::Failed | ServiceState::Backoff => {
            step(services, state, TransitionCause::ParentSetupFailure)
        }
        ServiceState::Stopping => step(services, state, TransitionCause::ExplicitStop),
        other => panic!("test helper does not walk to {other:?}"),
    }
}

/// The states in which a service still has, or is about to have again, a
/// process on its terminal. Pinned as a set so that adding a state to
/// `ServiceState` forces a decision about it here.
#[test]
fn possession_covers_every_state_with_a_process_coming_or_going() {
    for state in [
        ServiceState::Starting,
        ServiceState::Active,
        ServiceState::Reloading,
        ServiceState::Stopping,
        ServiceState::Backoff,
    ] {
        assert!(holds_tty(state), "{state:?} must hold the terminal");
    }
    for state in [
        ServiceState::Inactive,
        ServiceState::Completed,
        ServiceState::Failed,
        ServiceState::Skipped,
        ServiceState::Abandoned,
    ] {
        assert!(!holds_tty(state), "{state:?} must release the terminal");
    }
}

/// The rule the whole queue rests on: a service waiting out a restart delay
/// has not let go. Handing its console away during the gap produces two owners
/// a second later.
#[test]
fn a_service_in_backoff_has_not_released_its_terminal() {
    let mut services = table(vec![terminal_service("oobe", 100)]);
    set_state(&mut services, "oobe", ServiceState::Backoff);

    assert_eq!(tty_holder(&services, CONSOLE, "login"), Some("oobe"));
}

#[test]
fn a_finished_service_releases_its_terminal() {
    let mut services = table(vec![terminal_service("oobe", 100)]);
    set_state(&mut services, "oobe", ServiceState::Completed);

    assert_eq!(tty_holder(&services, CONSOLE, "login"), None);
}

/// A service restarting must not find its own previous incarnation in the way.
#[test]
fn a_service_does_not_hold_its_terminal_against_itself() {
    let mut services = table(vec![terminal_service("oobe", 100)]);
    set_state(&mut services, "oobe", ServiceState::Active);

    assert_eq!(tty_holder(&services, CONSOLE, "oobe"), None);
}

#[test]
fn another_terminal_is_not_this_terminal() {
    let mut oobe = terminal_service("oobe", 100);
    oobe.console_path = Some("/dev/tty2".to_string());
    let mut services = table(vec![oobe]);
    set_state(&mut services, "oobe", ServiceState::Active);

    assert_eq!(tty_holder(&services, CONSOLE, "login"), None);
}

#[test]
fn the_highest_precedence_waiter_takes_the_terminal() {
    let services = table(vec![
        terminal_service("login-console", 0),
        terminal_service("oobe", 100),
    ]);

    assert_eq!(tty_release_candidate(&services, CONSOLE, &[]), Some("oobe"));
}

/// An operator who states no preference still gets the same machine on every
/// boot, rather than whichever definition the registry enumerated first.
#[test]
fn equal_precedence_breaks_on_name() {
    let services = table(vec![
        terminal_service("bravo", 5),
        terminal_service("alpha", 5),
    ]);

    assert_eq!(
        tty_release_candidate(&services, CONSOLE, &[]),
        Some("alpha")
    );
}

/// The queue is opt-in. A terminal service that never asked to be woken this
/// way is not conscripted into waiting for one.
#[test]
fn a_service_without_the_trigger_is_not_a_candidate() {
    let mut login = terminal_service("login-console", 0);
    login.triggers = vec![ServiceTrigger::BootSettled];
    let services = table(vec![login]);

    assert_eq!(tty_release_candidate(&services, CONSOLE, &[]), None);
}

/// Disabled suppresses automatic activation by any trigger, and this is one.
#[test]
fn a_disabled_service_is_not_a_candidate() {
    let mut login = terminal_service("login-console", 0);
    login.disabled = true;
    let services = table(vec![login]);

    assert_eq!(tty_release_candidate(&services, CONSOLE, &[]), None);
}

/// The case that makes the release worth firing at all: the loser was skipped
/// when it lost the terminal, and skipped is a state you can be woken out of.
#[test]
fn a_skipped_waiter_is_still_a_candidate() {
    let mut services = table(vec![terminal_service("login-console", 0)]);
    set_state(&mut services, "login-console", ServiceState::Skipped);

    assert_eq!(
        tty_release_candidate(&services, CONSOLE, &[]),
        Some("login-console")
    );
}

/// The service whose exit freed the terminal does not get it straight back.
///
/// It is the one with the strongest claim on its own console, so without this
/// the highest-precedence holder restarts itself on every exit — a restart
/// policy with no budget, and one no waiter behind it ever gets past.
#[test]
fn the_service_that_released_the_terminal_is_not_offered_it_back() {
    let mut services = table(vec![
        terminal_service("login-console", 0),
        terminal_service("oobe", 100),
    ]);
    set_state(&mut services, "oobe", ServiceState::Completed);

    assert_eq!(
        tty_release_candidate(&services, CONSOLE, &["oobe".to_string()]),
        Some("login-console"),
    );
}

/// With nobody else queued, an exit leaves the terminal idle rather than
/// relaunching the service that just left it.
#[test]
fn a_lone_waiter_releasing_its_own_terminal_starts_nothing() {
    let mut services = table(vec![terminal_service("oobe", 100)]);
    set_state(&mut services, "oobe", ServiceState::Completed);

    assert_eq!(
        tty_release_candidate(&services, CONSOLE, &["oobe".to_string()]),
        None,
    );
}

/// Nobody is offered a terminal they are already using.
#[test]
fn a_running_waiter_is_not_offered_the_terminal_again() {
    let mut services = table(vec![
        terminal_service("login-console", 0),
        terminal_service("oobe", 100),
    ]);
    set_state(&mut services, "oobe", ServiceState::Active);

    assert_eq!(
        tty_release_candidate(&services, CONSOLE, &[]),
        Some("login-console")
    );
}
