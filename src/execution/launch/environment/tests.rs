use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use crate::boundary::ProcessInheritedFd;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::job::{JobRecord, ServiceMainJobSpec};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;
use crate::service::ServiceEnvironmentVariable;

use super::{
    DEFAULT_PATH, LISTEN_FDNAMES, LISTEN_FDS, NOTIFY_SOCKET, PATH, build_launch_environment,
    build_launch_environment_with_inherited_fds,
};

#[test]
fn launch_environment_layers_base_service_and_protocol_variables() {
    let mut job = test_job();
    let global_environment = vec![
        ServiceEnvironmentVariable {
            name: PATH.to_string(),
            value: "/global/bin".to_string(),
        },
        ServiceEnvironmentVariable {
            name: "APP_MODE".to_string(),
            value: "global".to_string(),
        },
        ServiceEnvironmentVariable {
            name: "GLOBAL_ONLY".to_string(),
            value: "yes".to_string(),
        },
        ServiceEnvironmentVariable {
            name: NOTIFY_SOCKET.to_string(),
            value: "/global.sock".to_string(),
        },
    ];
    job.environment = vec![
        ServiceEnvironmentVariable {
            name: PATH.to_string(),
            value: "/custom/bin".to_string(),
        },
        ServiceEnvironmentVariable {
            name: "APP_MODE".to_string(),
            value: "prod".to_string(),
        },
        ServiceEnvironmentVariable {
            name: NOTIFY_SOCKET.to_string(),
            value: "/ignored.sock".to_string(),
        },
    ];
    let environment = build_launch_environment_with_inherited_fds(
        &job,
        "/run/test/notify.sock",
        &global_environment,
        &[],
    );

    assert_eq!(
        environment
            .iter()
            .map(|variable| (variable.name.as_str(), variable.value.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("APP_MODE", "prod"),
            ("GLOBAL_ONLY", "yes"),
            (NOTIFY_SOCKET, "/run/test/notify.sock"),
            (PATH, "/custom/bin"),
        ],
    );
}

#[test]
fn launch_environment_defaults_path_when_service_does_not_override_it() {
    let job = test_job();
    let environment = build_launch_environment(&job, "/run/test/notify.sock");

    assert_eq!(
        environment
            .iter()
            .find(|variable| variable.name == PATH)
            .map(|variable| variable.value.as_str()),
        Some(DEFAULT_PATH),
    );
}

#[test]
fn launch_environment_skips_global_env_for_early_platform_services() {
    let job = test_job_for_service("authd");
    let global_environment = vec![
        ServiceEnvironmentVariable {
            name: PATH.to_string(),
            value: "/global/bin".to_string(),
        },
        ServiceEnvironmentVariable {
            name: "GLOBAL_ONLY".to_string(),
            value: "yes".to_string(),
        },
    ];

    let environment = build_launch_environment_with_inherited_fds(
        &job,
        "/run/test/notify.sock",
        &global_environment,
        &[],
    );

    assert_eq!(
        environment
            .iter()
            .find(|variable| variable.name == PATH)
            .map(|variable| variable.value.as_str()),
        Some(DEFAULT_PATH),
    );
    assert!(
        environment
            .iter()
            .all(|variable| variable.name != "GLOBAL_ONLY")
    );
}

#[test]
fn launch_environment_adds_fd_store_protocol_variables() {
    let mut job = test_job();
    job.environment = vec![
        ServiceEnvironmentVariable {
            name: LISTEN_FDS.to_string(),
            value: "ignored".to_string(),
        },
        ServiceEnvironmentVariable {
            name: LISTEN_FDNAMES.to_string(),
            value: "ignored".to_string(),
        },
    ];
    let inherited_fds = vec![inherited_fd("listener"), inherited_fd("cache")];

    let environment = build_launch_environment_with_inherited_fds(
        &job,
        "/run/test/notify.sock",
        &[],
        &inherited_fds,
    );

    assert_eq!(
        environment
            .iter()
            .filter(|variable| variable.name == LISTEN_FDS || variable.name == LISTEN_FDNAMES)
            .map(|variable| (variable.name.as_str(), variable.value.as_str()))
            .collect::<Vec<_>>(),
        vec![(LISTEN_FDNAMES, "listener:cache"), (LISTEN_FDS, "2")],
    );
}

fn test_job() -> JobRecord {
    test_job_for_service("app")
}

fn test_job_for_service(service: &str) -> JobRecord {
    let service = ServiceDefinition::simple_system_boot(service, "/sbin/app");
    JobRecord::new_service_main(
        JobIdAllocator::new().allocate_batch(1, 1).expect("job id")[0],
        ServiceMainJobSpec {
            service: &service,
            resolved_identity: "SYSTEM".to_string(),
            token_summary: TokenSummary::requested_identity("SYSTEM"),
            activation_generation: 1,
            cgroup_generation: 0,
            operation_id: OperationIdAllocator::new()
                .allocate_batch(1, 1)
                .expect("operation id")[0],
            created_at_ns: 1,
        },
    )
}

fn inherited_fd(name: &str) -> ProcessInheritedFd {
    ProcessInheritedFd {
        name: name.to_string(),
        fd: test_fd(),
    }
}

fn test_fd() -> OwnedFd {
    let (left, _right) = UnixStream::pair().expect("socket pair");
    left.into()
}

#[test]
fn default_path_uses_stratafs_runtime_views() {
    assert_eq!(DEFAULT_PATH, "/sbin:/bin");
}
