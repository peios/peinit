use crate::init::KernelCommandLine;

#[test]
fn kernel_command_line_parses_peios_boot_flags() {
    let parsed = KernelCommandLine::parse("quiet peios.safemode=1 root=/dev/vda peios.recovery=1");

    assert!(parsed.safe_mode);
    assert!(parsed.recovery);
}

#[test]
fn kernel_command_line_parses_console_flag() {
    let parsed = KernelCommandLine::parse("quiet peios.console=1 root=/dev/vda");

    assert!(parsed.console);
    assert!(!parsed.safe_mode);
    assert!(!parsed.recovery);
}

#[test]
fn kernel_command_line_console_defaults_off() {
    let parsed = KernelCommandLine::parse("quiet root=/dev/vda");

    assert!(!parsed.console);
}
