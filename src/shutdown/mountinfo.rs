use crate::boundary::BoundaryError;

pub(super) fn parse_mountinfo_mount_points(contents: &str) -> Result<Vec<String>, BoundaryError> {
    crate::mountinfo::parse_mountinfo_mount_points(contents).map_err(BoundaryError::Shutdown)
}

pub(super) fn mountinfo_contains_mount_point(
    contents: &str,
    mount_point: &str,
) -> Result<bool, BoundaryError> {
    crate::mountinfo::mountinfo_contains_mount_point(contents, mount_point)
        .map_err(BoundaryError::Shutdown)
}

#[cfg(test)]
mod tests {
    use super::{mountinfo_contains_mount_point, parse_mountinfo_mount_points};

    #[test]
    fn parses_and_decodes_mount_points_from_mountinfo() {
        let contents = "\
26 23 0:22 / /sys rw,nosuid,nodev,noexec,relatime - sysfs sysfs rw
27 23 0:5 / /dev rw,nosuid - devtmpfs devtmpfs rw,size=4096k
31 27 0:27 /pts /dev/pts rw,nosuid,noexec,relatime - devpts devpts rw
42 23 0:30 /space /run/a\\040b rw - tmpfs tmpfs rw
43 23 0:31 /slash /run/a\\134b rw - tmpfs tmpfs rw
";

        let mount_points = parse_mountinfo_mount_points(contents).expect("mountinfo");

        assert_eq!(
            mount_points,
            vec!["/sys", "/dev", "/dev/pts", "/run/a b", "/run/a\\b"],
        );
    }

    #[test]
    fn detects_whether_mount_point_is_still_present() {
        let contents = "26 23 0:22 / /sys rw - sysfs sysfs rw\n";

        assert!(mountinfo_contains_mount_point(contents, "/sys").expect("present"));
        assert!(!mountinfo_contains_mount_point(contents, "/run").expect("absent"));
    }

    #[test]
    fn rejects_malformed_mountinfo() {
        let error = parse_mountinfo_mount_points("26 23 0:22 /").expect_err("invalid");

        assert!(format!("{error:?}").contains("missing mount point field"));
    }
}
