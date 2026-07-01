use crate::execution::command::ExecutableCommandParseError;
use crate::registry::{ServiceRegistryDecodeError, build_service_definition_from_registry_values};

use super::super::{multi_sz, sz};

#[test]
fn malformed_success_exit_codes_are_rejected() {
    let err = build_service_definition_from_registry_values(
        "broken",
        &[
            sz("ImagePath", "/usr/bin/app"),
            multi_sz("SuccessExitCodes", &["256"]),
        ],
    )
    .expect_err("invalid success exit code");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidSuccessExitCode {
            value: "256".to_string(),
        }
    );
}

#[test]
fn malformed_environment_variables_are_rejected() {
    let err = build_service_definition_from_registry_values(
        "broken",
        &[
            sz("ImagePath", "/usr/bin/app"),
            multi_sz("Environment", &["MISSING_EQUALS"]),
        ],
    )
    .expect_err("invalid environment");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidEnvironmentVariable {
            value: "MISSING_EQUALS".to_string(),
        }
    );
}

#[test]
fn missing_image_path_is_rejected() {
    let err = build_service_definition_from_registry_values("broken", &[])
        .expect_err("missing image path");

    assert_eq!(err, ServiceRegistryDecodeError::MissingImagePath);
}

#[test]
fn invalid_service_names_are_rejected() {
    let err = build_service_definition_from_registry_values(
        "bad/name",
        &[sz("ImagePath", "/usr/bin/app")],
    )
    .expect_err("invalid service name");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidServiceName {
            service: "bad/name".to_string(),
        }
    );
}

#[test]
fn duplicate_known_fields_are_rejected_case_insensitively() {
    let err = build_service_definition_from_registry_values(
        "app",
        &[sz("ImagePath", "/usr/bin/app"), sz("imagepath", "/bin/app")],
    )
    .expect_err("duplicate image path");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::DuplicateField { field: "ImagePath" }
    );
}

#[test]
fn service_reference_fields_reject_invalid_names() {
    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            multi_sz("Requires", &["valid", "bad:name"]),
        ],
    )
    .expect_err("invalid dependency name");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidServiceReference {
            field: "Requires",
            value: "bad:name".to_string(),
        }
    );
}

#[test]
fn malformed_known_triggers_are_rejected() {
    for trigger in ["", "boot:anything", "timer", "timer:", ":event"] {
        let err = build_service_definition_from_registry_values(
            "app",
            &[
                sz("ImagePath", "/usr/bin/app"),
                multi_sz("Triggers", &[trigger]),
            ],
        )
        .expect_err("invalid trigger");

        assert_eq!(
            err,
            ServiceRegistryDecodeError::InvalidTrigger {
                value: trigger.to_string(),
            }
        );
    }
}

#[test]
fn path_fields_must_be_absolute() {
    let err =
        build_service_definition_from_registry_values("app", &[sz("ImagePath", "relative/app")])
            .expect_err("relative image path");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidAbsolutePath {
            field: "ImagePath",
            value: "relative/app".to_string(),
        }
    );

    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            sz("WorkingDirectory", "srv/app"),
        ],
    )
    .expect_err("relative working directory");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidAbsolutePath {
            field: "WorkingDirectory",
            value: "srv/app".to_string(),
        }
    );
}

#[test]
fn runtime_directories_must_be_single_relative_components() {
    for value in [
        "",
        ".",
        "..",
        "/app",
        "app/cache",
        "app\\cache",
        "bad\nname",
    ] {
        let err = build_service_definition_from_registry_values(
            "app",
            &[
                sz("ImagePath", "/usr/bin/app"),
                multi_sz("RuntimeDirectories", &[value]),
            ],
        )
        .expect_err("invalid runtime directory");

        assert_eq!(
            err,
            ServiceRegistryDecodeError::InvalidRuntimeDirectory {
                field: "RuntimeDirectories",
                value: value.to_string(),
            }
        );
    }
}

#[test]
fn executable_command_fields_are_validated() {
    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            multi_sz("ExecStartPre", &["relative-hook"]),
        ],
    )
    .expect_err("relative hook");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidExecutableCommand {
            field: "ExecStartPre",
            value: "relative-hook".to_string(),
            source: ExecutableCommandParseError::RelativeExecutable {
                executable: "relative-hook".to_string(),
            },
        }
    );

    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            sz("HealthCheck", "/usr/bin/check \"unterminated"),
        ],
    )
    .expect_err("unterminated health check");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidExecutableCommand {
            field: "HealthCheck",
            value: "/usr/bin/check \"unterminated".to_string(),
            source: ExecutableCommandParseError::UnclosedDoubleQuote,
        }
    );
}

#[test]
fn exec_reload_signal_must_be_canonical_and_allowed() {
    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            sz("ExecReload", "signal:sighup"),
        ],
    )
    .expect_err("lowercase reload signal");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidReloadSignal {
            value: "signal:sighup".to_string(),
        }
    );

    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            sz("ExecReload", "signal:SIGKILL"),
        ],
    )
    .expect_err("unhandleable reload signal");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidReloadSignal {
            value: "signal:SIGKILL".to_string(),
        }
    );
}

#[test]
fn conditions_and_asserts_validate_check_syntax_and_registry_cache_scope() {
    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            multi_sz("Conditions", &["unknown:/thing"]),
        ],
    )
    .expect_err("unknown condition kind");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidCheck {
            field: "Conditions",
            value: "unknown:/thing".to_string(),
        }
    );

    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            multi_sz("Asserts", &["registry:Machine\\Software\\Other"]),
        ],
    )
    .expect_err("non cached registry assert");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::NonCachedRegistryCheck {
            field: "Asserts",
            key: "Machine\\Software\\Other".to_string(),
        }
    );
}

#[test]
fn required_privileges_reject_empty_entries() {
    let err = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/usr/bin/app"),
            multi_sz("RequiredPrivileges", &[""]),
        ],
    )
    .expect_err("empty privilege");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidListEntry {
            field: "RequiredPrivileges",
            value: String::new(),
        }
    );
}
