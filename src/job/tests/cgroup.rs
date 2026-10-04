use crate::job::{
    CgroupMember, ServiceCgroupKind, ServicePart, cgroup_member, encode_service_cgroup_id,
    service_cgroup_root_path, service_job_cgroup_path,
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

/// `/proc/<pid>/cgroup` for a process in the cgroup at `path`, which peinit
/// names under `/sys/fs/cgroup`.
fn proc_cgroup(path: &str) -> String {
    format!("0::{}\n", path.strip_prefix("/sys/fs/cgroup").unwrap())
}

#[test]
fn a_process_s_cgroup_names_its_service_and_part_whatever_the_name_or_generation() {
    for name in ["sshd", "a/b %", "app.gen1", "app%gen1", "jobs"] {
        for generation in [0, 3] {
            for (kind, part) in [
                (ServiceCgroupKind::Main, ServicePart::Main),
                (ServiceCgroupKind::Hooks, ServicePart::Hooks),
                (ServiceCgroupKind::Health, ServicePart::Health),
            ] {
                let path = service_job_cgroup_path(name, generation, kind);
                assert_eq!(
                    cgroup_member(&proc_cgroup(&path)),
                    Some(CgroupMember::Service { service: name.to_string(), part: Some(part) }),
                    "{path}",
                );
            }
        }
    }
    let checks = format!("{}/checks", service_cgroup_root_path("sshd", 0));
    assert_eq!(
        cgroup_member(&proc_cgroup(&checks)),
        Some(CgroupMember::Service { service: "sshd".into(), part: Some(ServicePart::Checks) }),
    );
}

#[test]
fn a_submitted_job_s_process_names_its_job_and_others_name_nothing() {
    let id = "4f7a1c2e-9b3d-4e5f-8a6b-0c1d2e3f4a5b";
    assert_eq!(
        cgroup_member(&format!("0::/peinit/jobs/{id}\n")),
        Some(CgroupMember::Job(id.to_string())),
    );
    // peinit itself, and a kernel thread.
    assert_eq!(cgroup_member("0::/\n"), None);
    assert_eq!(cgroup_member("0::/init.scope\n"), None);
    assert_eq!(cgroup_member(""), None);
    // A broken escape is no service.
    assert_eq!(cgroup_member("0::/peinit/a%2/main\n"), None);
}
