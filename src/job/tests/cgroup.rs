use crate::job::{
    ServiceCgroupKind, encode_service_cgroup_id, service_cgroup_root_path, service_job_cgroup_path,
};

#[test]
fn cgroup_paths_use_injective_percent_encoding_and_generations() {
    assert_eq!(encode_service_cgroup_id("safe.Name-1"), "safe.Name-1");
    assert_eq!(encode_service_cgroup_id("a/b %"), "a%2Fb%20%25");
    assert_eq!(
        service_cgroup_root_path("app", 0),
        "/sys/fs/cgroup/peinit/app",
    );
    assert_eq!(
        service_cgroup_root_path("app", 2),
        "/sys/fs/cgroup/peinit/app%gen2",
    );
    assert_eq!(
        service_job_cgroup_path("app", 2, ServiceCgroupKind::Main),
        "/sys/fs/cgroup/peinit/app%gen2/main",
    );
}

// PEI-353. The encoding's injectivity argument covers the *id*; it did not
// extend to the generational suffix appended afterwards. `.` is in the safe
// set and service names permit it, so a service literally named `app.gen1`
// shared a tree with generation 1 of a service named `app` — one service's
// processes joining another's leaked tree.
//
// `%g` cannot occur inside an encoded id: `%` self-escapes, and the encoder
// only ever emits it followed by two uppercase hex digits. So the first `%g`
// in a path separates id from generation, and the pair is recoverable.
#[test]
fn a_generational_path_cannot_collide_with_a_service_named_like_one() {
    assert_ne!(
        service_cgroup_root_path("app.gen1", 0),
        service_cgroup_root_path("app", 1),
    );
    // Nor by naming the separator itself: `%` is escaped on the way in.
    assert_eq!(
        service_cgroup_root_path("app%gen1", 0),
        "/sys/fs/cgroup/peinit/app%25gen1",
    );
    assert_ne!(
        service_cgroup_root_path("app%gen1", 0),
        service_cgroup_root_path("app", 1),
    );
}
