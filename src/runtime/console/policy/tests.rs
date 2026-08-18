use super::*;
use crate::service::{ServiceDefinition, ServiceTable};

fn policy(level: QuietLevel, owned: bool) -> QuietPolicy {
    QuietPolicy::new(level, owned)
}

fn console_service(name: &str) -> ServiceDefinition {
    let mut definition = ServiceDefinition::simple_system_boot(name, "/bin/login");
    definition.console_path = Some(CONSOLE_PATH.to_string());
    definition
}

/// Nothing owns the console: every severity is written at both the default and
/// the verbose level.
#[test]
fn an_unowned_console_carries_everything_below_blackout() {
    for level in [QuietLevel::Verbose, QuietLevel::Standard] {
        let policy = policy(level, false);
        assert!(policy.allows(ConsoleSeverity::Status), "{level:?}");
        assert!(policy.allows(ConsoleSeverity::Error), "{level:?}");
        assert!(policy.allows(ConsoleSeverity::Critical), "{level:?}");
    }
}

/// Blackout silences the narrative, not the news.
#[test]
fn blackout_drops_status_and_keeps_errors() {
    let policy = policy(QuietLevel::Blackout, false);

    assert!(!policy.allows(ConsoleSeverity::Status));
    assert!(policy.allows(ConsoleSeverity::Error));
    assert!(policy.allows(ConsoleSeverity::Critical));
}

/// Ownership is the stricter gate: an error is worth overriding a *preference*
/// for silence, but not worth writing into someone else's live prompt.
#[test]
fn an_owned_console_keeps_only_critical_messages() {
    let policy = policy(QuietLevel::Standard, true);

    assert!(!policy.allows(ConsoleSeverity::Status));
    assert!(!policy.allows(ConsoleSeverity::Error));
    assert!(policy.allows(ConsoleSeverity::Critical));
}

/// The two rules stack, so errors are never *less* visible at Blackout than at
/// Standard — which is what makes the levels coherent rather than a scale with
/// a hole in it.
#[test]
fn blackout_is_never_quieter_about_errors_than_standard() {
    for owned in [false, true] {
        let standard = policy(QuietLevel::Standard, owned);
        let blackout = policy(QuietLevel::Blackout, owned);
        for severity in [
            ConsoleSeverity::Status,
            ConsoleSeverity::Error,
            ConsoleSeverity::Critical,
        ] {
            if blackout.allows(severity) {
                assert!(
                    standard.allows(severity),
                    "{severity:?} at owned={owned} passes Blackout but not Standard",
                );
            }
        }
    }
}

/// `peios.quiet=0` is the bring-up setting: it turns the ownership rule off, so
/// the operator keeps peinit's narrative even at the cost of a scrambled
/// prompt.
#[test]
fn verbose_writes_into_an_owned_console() {
    assert!(!QuietLevel::Verbose.respects_terminal_ownership());

    let services = ServiceTable::from_boot_snapshot(vec![console_service("login-console")])
        .expect("service table");
    let policy = QuietPolicy::evaluate(QuietLevel::Verbose, &services);

    assert!(policy.allows(ConsoleSeverity::Status));
}

/// A service that declares no terminal cannot silence peinit.
#[test]
fn a_service_without_a_terminal_does_not_own_the_console() {
    let services = ServiceTable::from_boot_snapshot(vec![ServiceDefinition::simple_system_boot(
        "app",
        "/sbin/app",
    )])
    .expect("service table");

    let policy = QuietPolicy::evaluate(QuietLevel::Standard, &services);

    assert!(policy.allows(ConsoleSeverity::Status));
}

/// A defined but not-yet-running terminal service holds nothing: it has no
/// process, so there is no session to corrupt.
#[test]
fn an_inactive_terminal_service_does_not_own_the_console() {
    let services = ServiceTable::from_boot_snapshot(vec![console_service("login-console")])
        .expect("service table");

    let policy = QuietPolicy::evaluate(QuietLevel::Standard, &services);

    assert!(policy.allows(ConsoleSeverity::Status));
}
