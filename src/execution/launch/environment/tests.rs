use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use crate::boundary::ProcessInheritedFd;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::job::{JobRecord, ServiceMainJobSpec};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;
use crate::service::ServiceEnvironmentVariable;

use super::{
    DEFAULT_PATH, LISTEN_FDNAMES, LISTEN_FDS, LISTEN_PID, NOTIFY_SOCKET, PATH,
    build_launch_environment, build_launch_environment_with_inherited_fds,
};

#[test]
fn default_path_uses_stratafs_runtime_views() {
    assert_eq!(DEFAULT_PATH, "/sbin:/bin");
}

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
fn launch_environment_skips_global_env_for_registryd() {
    let job = test_job_for_service(ServiceDefinition::REGISTRYD_NAME);
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
fn launch_environment_applies_global_env_to_other_platform_daemons() {
    // authd, lpsd, eventd and eudev used to be exempted alongside registryd on
    // the stated grounds that the registry "is not yet readable when they
    // start". It is: they are Phase 2 services, launched after
    // `run_phase2_boot` has read the key into supervisor state. Only registryd
    // — which serves the key — is exempt, so these get the ordinary layering.
    for service in ["authd", "lpsd", "eventd", "eudev"] {
        let job = test_job_for_service(service);
        let global_environment = vec![ServiceEnvironmentVariable {
            name: "GLOBAL_ONLY".to_string(),
            value: "yes".to_string(),
        }];

        let environment = build_launch_environment_with_inherited_fds(
            &job,
            "/run/test/notify.sock",
            &global_environment,
            &[],
        );

        assert!(
            environment
                .iter()
                .any(|variable| variable.name == "GLOBAL_ONLY"),
            "{service} should receive the global EnvVars layer",
        );
    }
}

#[test]
fn launch_environment_applies_global_env_to_non_system_registryd() {
    // The SYSTEM check is load-bearing: the exemption is for the platform's
    // registry daemon, not for anything wearing its name.
    let service = ServiceDefinition::simple_system_boot(
        ServiceDefinition::REGISTRYD_NAME,
        ServiceDefinition::REGISTRYD_IMAGE_PATH,
    );
    let job = JobRecord::new_service_main(
        JobIdAllocator::new().allocate_batch(1, 1).expect("job id")[0],
        ServiceMainJobSpec {
            service: &service,
            resolved_identity: "LocalService".to_string(),
            token_summary: TokenSummary::requested_identity("LocalService"),
            activation_generation: 1,
            cgroup_generation: 0,
            operation_id: OperationIdAllocator::new()
                .allocate_batch(1, 1)
                .expect("operation id")[0],
            created_at_ns: 1,
        },
    );
    let global_environment = vec![ServiceEnvironmentVariable {
        name: "GLOBAL_ONLY".to_string(),
        value: "yes".to_string(),
    }];

    let environment = build_launch_environment_with_inherited_fds(
        &job,
        "/run/test/notify.sock",
        &global_environment,
        &[],
    );

    assert!(
        environment
            .iter()
            .any(|variable| variable.name == "GLOBAL_ONLY")
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

#[test]
fn protocol_variables_from_a_configurable_layer_never_reach_the_child() {
    // PSPU §4.20: all four MUST be absent when nothing is passed, and no
    // configurable layer may set any of them.
    //
    // The un-injected case is the one that used to leak: with no descriptors,
    // the guarded LISTEN_* inserts never run, so nothing overwrote the
    // configurable layers. FdStoreMax defaults to 0, so that is every service.
    let names = [NOTIFY_SOCKET, LISTEN_FDS, LISTEN_FDNAMES, LISTEN_PID];

    for (label, from_global) in [("EnvVars", true), ("service Environment", false)] {
        for name in names {
            let mut job = test_job();
            let entry = vec![ServiceEnvironmentVariable {
                name: name.to_string(),
                value: "3".to_string(),
            }];
            let global: &[ServiceEnvironmentVariable] = if from_global { &entry } else { &[] };
            if !from_global {
                job.environment = entry.clone();
            }

            let environment = build_launch_environment_with_inherited_fds(
                &job,
                "/run/test/notify.sock",
                global,
                &[],
            );

            let value = environment
                .iter()
                .find(|variable| variable.name == name)
                .map(|variable| variable.value.as_str());

            match name {
                // Inserted unconditionally by peinit, so it is present — but
                // it must carry peinit's value, not the layer's.
                NOTIFY_SOCKET => assert_eq!(
                    value,
                    Some("/run/test/notify.sock"),
                    "{label} overrode {name}"
                ),
                _ => assert_eq!(value, None, "{label} smuggled {name} through with no fds"),
            }
        }
    }
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
