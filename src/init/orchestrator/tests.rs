use crate::init::KernelCommandLine;

#[test]
fn kernel_command_line_parses_peios_boot_flags() {
    let parsed = KernelCommandLine::parse("quiet peios.safemode=1 root=/dev/vda peios.recovery=1");

    assert!(parsed.safe_mode);
    assert!(parsed.recovery);
}
