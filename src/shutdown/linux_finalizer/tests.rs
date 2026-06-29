use std::path::PathBuf;

use crate::boundary::ShutdownFinalizer;

use super::LinuxShutdownFinalizer;

#[test]
fn snapshots_mounts_from_configured_mountinfo_path() {
    let path = temp_mountinfo_path();
    std::fs::write(
        &path,
        "\
26 23 0:22 / /sys rw,nosuid,nodev,noexec,relatime - sysfs sysfs rw
27 23 0:30 /space /run/a\\040b rw - tmpfs tmpfs rw
",
    )
    .expect("write mountinfo fixture");

    let mut finalizer = LinuxShutdownFinalizer::with_mountinfo_path(&path);

    assert_eq!(
        finalizer.snapshot_mounts().expect("snapshot mounts"),
        vec!["/sys", "/run/a b"],
    );
    let _ = std::fs::remove_file(path);
}

fn temp_mountinfo_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "peinit2-mountinfo-{}-{}",
        std::process::id(),
        unique_suffix(),
    ))
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos()
}
