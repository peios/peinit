use crate::init::{KernelCommandLine, QuietLevel};

#[test]
fn kernel_command_line_parses_peios_boot_flags() {
    let parsed = KernelCommandLine::parse("quiet peios.safemode=1 root=/dev/vda peios.recovery=1");

    assert!(parsed.safe_mode);
    assert!(parsed.recovery);
}

#[test]
fn kernel_command_line_boot_flags_default_off() {
    let parsed = KernelCommandLine::parse("quiet root=/dev/vda");

    assert!(!parsed.safe_mode);
    assert!(!parsed.recovery);
    assert_eq!(parsed.boot_attempt_threshold, None);
    assert_eq!(parsed.notify_socket_path, None);
    assert_eq!(parsed.quiet, QuietLevel::Standard);
}

/// Service selection is not a command-line concern. `peios.console=1` and
/// `peios.login=1` used to inject compiled-in definitions into the boot set;
/// they are ordinary registry services now, so these tokens are simply unknown
/// and ignored like any other.
#[test]
fn retired_service_injection_flags_are_ignored() {
    let parsed = KernelCommandLine::parse("peios.console=1 peios.login=1");

    assert!(!parsed.safe_mode);
    assert!(!parsed.recovery);
    assert_eq!(parsed.boot_attempt_threshold, None);
    assert_eq!(parsed.notify_socket_path, None);
}

#[test]
fn kernel_command_line_parses_boot_attempt_threshold() {
    let parsed = KernelCommandLine::parse("quiet peios.bootattempts=7 root=/dev/vda");

    assert_eq!(parsed.boot_attempt_threshold, Some(7));
}

/// `0` is meaningful, not absent: it disables the recovery trigger for a system
/// whose boot-attempt counter is itself the thing that is wrong.
#[test]
fn boot_attempt_threshold_of_zero_is_distinct_from_unset() {
    assert_eq!(
        KernelCommandLine::parse("peios.bootattempts=0").boot_attempt_threshold,
        Some(0),
    );
    assert_eq!(KernelCommandLine::parse("").boot_attempt_threshold, None,);
}

/// A typo in a tuning knob must not stop the machine booting: this parser runs
/// before anything exists to report a diagnostic to.
#[test]
fn malformed_boot_attempt_threshold_falls_back_to_the_default() {
    for line in [
        "peios.bootattempts=",
        "peios.bootattempts=many",
        "peios.bootattempts=-1",
        "peios.bootattempts=99999999999999999999",
    ] {
        assert_eq!(
            KernelCommandLine::parse(line).boot_attempt_threshold,
            None,
            "{line} should be ignored, not fatal",
        );
    }
}

#[test]
fn kernel_command_line_parses_notify_socket_path() {
    let parsed = KernelCommandLine::parse("quiet peios.notifysocket=/run/alt/notify.sock");

    assert_eq!(
        parsed.notify_socket_path.as_deref(),
        Some("/run/alt/notify.sock"),
    );
}

#[test]
fn empty_notify_socket_path_is_ignored() {
    assert_eq!(
        KernelCommandLine::parse("peios.notifysocket=").notify_socket_path,
        None,
    );
}

/// Matches the kernel's own handling of a repeated parameter.
#[test]
fn repeated_valued_tokens_take_the_last_occurrence() {
    let parsed = KernelCommandLine::parse(
        "peios.bootattempts=2 peios.notifysocket=/run/a.sock \
         peios.bootattempts=9 peios.notifysocket=/run/b.sock",
    );

    assert_eq!(parsed.boot_attempt_threshold, Some(9));
    assert_eq!(parsed.notify_socket_path.as_deref(), Some("/run/b.sock"));
}

/// A `peios.*` token peinit does not know must be ignored rather than
/// misparsed — the kernel command line is shared with the kernel and every
/// other early consumer.
#[test]
fn unknown_tokens_are_ignored() {
    let parsed = KernelCommandLine::parse(
        "BOOT_IMAGE=/vmlinuz root=UUID=abc ro quiet peios.notaflag=1 peios.safemode",
    );

    assert!(!parsed.safe_mode);
    assert!(!parsed.recovery);
}

/// The three quiet levels. Not a scale: 0 turns the terminal-ownership rule
/// off, 1 leaves it on, and 2 adds a blackout of ordinary progress on top —
/// so 2 is never quieter about errors than 1.
#[test]
fn kernel_command_line_parses_quiet_levels() {
    for (token, expected) in [
        ("peios.quiet=0", QuietLevel::Verbose),
        ("peios.quiet=1", QuietLevel::Standard),
        ("peios.quiet=2", QuietLevel::Blackout),
    ] {
        assert_eq!(KernelCommandLine::parse(token).quiet, expected, "{token}");
    }
}

#[test]
fn quiet_defaults_to_standard() {
    assert_eq!(
        KernelCommandLine::parse("quiet root=/dev/vda").quiet,
        QuietLevel::Standard,
    );
}

/// Only level 0 lets peinit write into a terminal a service owns; only level 2
/// drops ordinary progress. Pinned because the two rules are independent and a
/// future level must not silently collapse them.
#[test]
fn the_two_quiet_rules_are_independent() {
    assert!(!QuietLevel::Verbose.respects_terminal_ownership());
    assert!(QuietLevel::Standard.respects_terminal_ownership());
    assert!(QuietLevel::Blackout.respects_terminal_ownership());

    assert!(!QuietLevel::Verbose.suppresses_status());
    assert!(!QuietLevel::Standard.suppresses_status());
    assert!(QuietLevel::Blackout.suppresses_status());
}

/// A typo in a logging knob must not decide how the machine boots.
#[test]
fn a_malformed_quiet_level_keeps_the_default() {
    for line in [
        "peios.quiet=",
        "peios.quiet=3",
        "peios.quiet=yes",
        "peios.quiet=-1",
    ] {
        assert_eq!(
            KernelCommandLine::parse(line).quiet,
            QuietLevel::Standard,
            "{line}",
        );
    }
}
