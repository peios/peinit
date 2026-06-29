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
        "/sys/fs/cgroup/peinit/app.gen2",
    );
    assert_eq!(
        service_job_cgroup_path("app", 2, ServiceCgroupKind::Main),
        "/sys/fs/cgroup/peinit/app.gen2/main",
    );
}
