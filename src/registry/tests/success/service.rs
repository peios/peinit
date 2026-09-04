use crate::provisioning::ServiceRuntimeDirectory;
use crate::registry::{
    apply_inherited_service_security, build_service_definition_from_registry_values,
};
use crate::service::{
    ErrorControl, NotifyAccess, Readiness, RestartPolicy, ServiceCheck, ServiceCheckKind,
    ServiceEnvironmentVariable, ServiceSecurityDescriptor, ServiceTrigger, ServiceType,
};

use super::super::{RawRegistryValue, RegistryValueType, binary, dword, multi_sz, sz};

#[test]
fn builds_service_definition_from_registry_values() {
    let definition = build_service_definition_from_registry_values(
        "app",
        &[
            sz("ImagePath", "/bin/app"),
            multi_sz("Arguments", &["--foreground"]),
            dword("Type", 1),
            multi_sz("Triggers", &["boot", "timer:daily", "event:custom"]),
            dword("Disabled", 0),
            dword("SafeMode", 1),
            sz("Identity", "SYSTEM"),
            multi_sz("RequiredPrivileges", &["SeChangeNotifyPrivilege"]),
            multi_sz("Requires", &["authd"]),
            multi_sz("Wants", &["eventd"]),
            multi_sz("BindsTo", &["network"]),
            multi_sz("Conflicts", &["legacy"]),
            sz("OnFailure", "fallback"),
            dword("Readiness", 1),
            dword("NotifyAccess", 0),
            dword("RemainAfterExit", 1),
            multi_sz("SuccessExitCodes", &["0", "2", "2"]),
            multi_sz("ExecStartPre", &["/bin/pre-one", "/bin/pre-two"]),
            multi_sz("ExecStartPost", &["/bin/post --flag"]),
            sz("HookIdentity", "SYSTEM"),
            sz("ExecReload", "signal:SIGUSR1"),
            dword("PreStartCheckTimeout", 7),
            dword("StartTimeout", 45),
            dword("StopTimeout", 12),
            dword("WatchdogTimeout", 60),
            sz("HealthCheck", "/bin/health --once"),
            dword("HealthCheckInterval", 10),
            dword("HealthCheckTimeout", 2),
            dword("HealthCheckRetries", 3),
            dword("FdStoreMax", 8),
            multi_sz("Environment", &["APP_MODE=prod", "EMPTY="]),
            sz("WorkingDirectory", "/srv/app"),
            multi_sz("RuntimeDirectories", &["app", "app-cache"]),
            dword("LimitNOFILE", 4096),
            dword("LimitCORE", 0),
            multi_sz(
                "Conditions",
                &["path:/srv/app", "registry:Machine\\System\\Services\\app"],
            ),
            multi_sz("Asserts", &["file:/bin/app", "directory:/srv"]),
            sz("DisplayName", "Application"),
            sz("Description", "Example application service"),
            dword("RestartPolicy", 2),
            dword("RestartMaxRetries", 9),
            dword("RestartWindow", 240),
            dword("RestartDelay", 3),
            dword("ErrorControl", 1),
            binary("ServiceSecurity", &[0x01, 0x02, 0x03]),
            dword("TimerPersistent", 0),
            dword("TimerJitter", 17),
        ],
    )
    .expect("definition");

    assert_eq!(definition.name, "app");
    assert_eq!(definition.image_path, "/bin/app");
    assert_eq!(definition.arguments, vec!["--foreground"]);
    assert_eq!(definition.service_type, ServiceType::Oneshot);
    assert_eq!(
        definition.triggers,
        vec![
            ServiceTrigger::Boot,
            ServiceTrigger::Timer {
                schedule: "daily".to_string()
            },
            ServiceTrigger::Other("event:custom".to_string()),
        ]
    );
    assert!(!definition.disabled);
    assert!(definition.safe_mode);
    assert_eq!(definition.identity, "SYSTEM");
    assert_eq!(
        definition.required_privileges,
        vec!["SeChangeNotifyPrivilege"]
    );
    assert_eq!(definition.requires, vec!["authd"]);
    assert_eq!(definition.wants, vec!["eventd"]);
    assert_eq!(definition.binds_to, vec!["network"]);
    assert_eq!(definition.conflicts, vec!["legacy"]);
    assert_eq!(definition.on_failure, Some("fallback".to_string()));
    assert_eq!(definition.readiness, Readiness::Alive);
    assert_eq!(definition.notify_access, NotifyAccess::Main);
    assert!(definition.remain_after_exit);
    assert_eq!(definition.success_exit_codes, vec![0, 2]);
    assert_eq!(
        definition.exec_start_pre,
        vec!["/bin/pre-one", "/bin/pre-two"]
    );
    assert_eq!(definition.exec_start_post, vec!["/bin/post --flag"]);
    assert_eq!(definition.hook_identity, Some("SYSTEM".to_string()));
    assert_eq!(definition.exec_reload, Some("signal:SIGUSR1".to_string()));
    assert_eq!(definition.pre_start_check_timeout_secs, 7);
    assert_eq!(definition.start_timeout_secs, 45);
    assert_eq!(definition.stop_timeout_secs, 12);
    assert_eq!(definition.watchdog_timeout_secs, 60);
    assert_eq!(
        definition.health_check,
        Some("/bin/health --once".to_string())
    );
    assert_eq!(definition.health_check_interval_secs, 10);
    assert_eq!(definition.health_check_timeout_secs, 2);
    assert_eq!(definition.health_check_retries, 3);
    assert_eq!(definition.fd_store_max, 8);
    assert_eq!(
        definition.environment,
        vec![
            ServiceEnvironmentVariable {
                name: "APP_MODE".to_string(),
                value: "prod".to_string(),
            },
            ServiceEnvironmentVariable {
                name: "EMPTY".to_string(),
                value: String::new(),
            },
        ]
    );
    assert_eq!(definition.working_directory, "/srv/app");
    // Not in the value set above: a service without TTYPath gets daemon stdio.
    assert_eq!(definition.console_path, None);
    assert_eq!(
        definition.runtime_directories,
        vec![
            ServiceRuntimeDirectory {
                name: "app".to_string(),
            },
            ServiceRuntimeDirectory {
                name: "app-cache".to_string(),
            },
        ]
    );
    assert_eq!(definition.limit_nofile, Some(4096));
    assert_eq!(definition.limit_core, Some(0));
    assert_eq!(
        definition.conditions,
        vec![
            ServiceCheck {
                kind: ServiceCheckKind::Path,
                argument: "/srv/app".to_string(),
            },
            ServiceCheck {
                kind: ServiceCheckKind::Registry,
                argument: "Machine\\System\\Services\\app".to_string(),
            },
        ]
    );
    assert_eq!(
        definition.asserts,
        vec![
            ServiceCheck {
                kind: ServiceCheckKind::File,
                argument: "/bin/app".to_string(),
            },
            ServiceCheck {
                kind: ServiceCheckKind::Directory,
                argument: "/srv".to_string(),
            },
        ]
    );
    assert_eq!(definition.display_name, Some("Application".to_string()));
    assert_eq!(
        definition.description,
        Some("Example application service".to_string())
    );
    assert_eq!(definition.restart_policy, RestartPolicy::Always);
    assert_eq!(definition.restart_max_retries, 9);
    assert_eq!(definition.restart_window_secs, 240);
    assert_eq!(definition.restart_delay_secs, 3);
    assert_eq!(definition.error_control, ErrorControl::Critical);
    assert_eq!(
        definition.service_security,
        ServiceSecurityDescriptor::RegistryBinary(vec![0x01, 0x02, 0x03])
    );
    assert!(!definition.timer_persistent);
    assert_eq!(definition.timer_jitter_secs, 17);
}

#[test]
fn defaults_absent_optional_fields() {
    let definition = build_service_definition_from_registry_values(
        "minimal",
        &[sz("ImagePath", "/bin/minimal")],
    )
    .expect("definition");

    assert_eq!(definition.identity, "LocalService");
    assert!(definition.required_privileges.is_empty());
    assert_eq!(definition.service_type, ServiceType::Simple);
    assert_eq!(definition.readiness, Readiness::Notify);
    assert_eq!(definition.notify_access, NotifyAccess::Main);
    assert!(!definition.remain_after_exit);
    assert!(definition.success_exit_codes.is_empty());
    assert_eq!(definition.restart_policy, RestartPolicy::OnFailure);
    assert_eq!(definition.restart_max_retries, 5);
    assert_eq!(definition.restart_window_secs, 120);
    assert_eq!(definition.restart_delay_secs, 1);
    assert_eq!(definition.error_control, ErrorControl::Normal);
    assert!(definition.exec_start_pre.is_empty());
    assert!(definition.exec_start_post.is_empty());
    assert_eq!(definition.hook_identity, None);
    assert_eq!(definition.exec_reload, None);
    assert_eq!(definition.pre_start_check_timeout_secs, 5);
    assert_eq!(definition.start_timeout_secs, 30);
    assert_eq!(definition.stop_timeout_secs, 10);
    assert_eq!(definition.watchdog_timeout_secs, 0);
    assert_eq!(definition.health_check, None);
    assert_eq!(definition.health_check_interval_secs, 30);
    assert_eq!(definition.health_check_timeout_secs, 5);
    assert_eq!(definition.health_check_retries, 3);
    assert_eq!(definition.fd_store_max, 0);
    assert!(definition.environment.is_empty());
    assert_eq!(definition.working_directory, "/");
    assert!(definition.runtime_directories.is_empty());
    assert_eq!(definition.limit_nofile, None);
    assert_eq!(definition.limit_core, None);
    assert!(definition.conditions.is_empty());
    assert!(definition.asserts.is_empty());
    assert_eq!(definition.display_name, None);
    assert_eq!(definition.description, None);
    assert!(definition.triggers.is_empty());
    assert_eq!(
        definition.service_security,
        ServiceSecurityDescriptor::Default
    );
    assert!(definition.timer_persistent);
    assert_eq!(definition.timer_jitter_secs, 0);
}

#[test]
fn applies_inherited_service_security_to_services_without_explicit_value() {
    let mut explicit = build_service_definition_from_registry_values(
        "explicit",
        &[
            sz("ImagePath", "/bin/explicit"),
            binary("ServiceSecurity", &[1, 2, 3]),
        ],
    )
    .expect("explicit definition");
    let inherited = build_service_definition_from_registry_values(
        "inherited",
        &[sz("ImagePath", "/bin/inherited")],
    )
    .expect("inherited definition");
    let fallback = build_service_definition_from_registry_values(
        "fallback",
        &[sz("ImagePath", "/bin/fallback")],
    )
    .expect("fallback definition");

    let mut definitions = vec![explicit.clone(), inherited, fallback];
    apply_inherited_service_security(
        &mut definitions,
        Some(ServiceSecurityDescriptor::RegistryBinary(vec![9, 8, 7])),
    );
    explicit.service_security = ServiceSecurityDescriptor::RegistryBinary(vec![1, 2, 3]);

    assert_eq!(definitions[0], explicit);
    assert_eq!(
        definitions[1].service_security,
        ServiceSecurityDescriptor::RegistryBinary(vec![9, 8, 7]),
    );
    assert_eq!(
        definitions[2].service_security,
        ServiceSecurityDescriptor::RegistryBinary(vec![9, 8, 7]),
    );
}

#[test]
fn empty_identity_defaults_to_local_service() {
    let definition = build_service_definition_from_registry_values(
        "svc",
        &[
            sz("ImagePath", "/bin/svc"),
            sz("Identity", ""),
            sz("HookIdentity", ""),
            sz("DisplayName", ""),
            sz("Description", ""),
        ],
    )
    .expect("definition");

    assert_eq!(definition.identity, "LocalService");
    assert_eq!(definition.hook_identity, None);
    assert_eq!(definition.display_name, None);
    assert_eq!(definition.description, None);
}

#[test]
fn well_known_identities_are_case_insensitive_and_canonicalized() {
    let definition = build_service_definition_from_registry_values(
        "svc",
        &[
            sz("ImagePath", "/bin/svc"),
            sz("Identity", "system"),
            sz("HookIdentity", "networkservice"),
        ],
    )
    .expect("definition");

    assert_eq!(definition.identity, "SYSTEM");
    assert_eq!(definition.hook_identity, Some("NetworkService".to_string()));
}

#[test]
fn non_well_known_identity_names_are_preserved_for_authd() {
    let definition = build_service_definition_from_registry_values(
        "svc",
        &[
            sz("ImagePath", "/bin/svc"),
            sz("Identity", "Domain\\BuildUser"),
            sz("HookIdentity", "local\\HookUser"),
        ],
    )
    .expect("definition");

    assert_eq!(definition.identity, "Domain\\BuildUser");
    assert_eq!(
        definition.hook_identity,
        Some("local\\HookUser".to_string())
    );
}

#[test]
fn critical_error_control_implies_safe_mode() {
    let definition = build_service_definition_from_registry_values(
        "svc",
        &[sz("ImagePath", "/bin/svc"), dword("ErrorControl", 1)],
    )
    .expect("definition");

    assert!(definition.safe_mode);
}

#[test]
fn unknown_optional_fields_are_ignored() {
    let definition = build_service_definition_from_registry_values(
        "svc",
        &[
            sz("ImagePath", "/bin/svc"),
            RawRegistryValue {
                name: "FutureField".to_string(),
                value_type: RegistryValueType::Binary,
                data: vec![1, 2, 3],
            },
        ],
    )
    .expect("definition");

    assert_eq!(definition.name, "svc");
    assert_eq!(definition.image_path, "/bin/svc");
}

/// TTYPath is what makes a console or login service expressible as registry
/// data. It replaced a compiled-in `attach_console` flag that only peinit's own
/// definitions could set, and it carries a path rather than a boolean so a
/// service can take `tty1` while the kernel console is a serial line.
#[test]
fn tty_path_attaches_a_named_terminal() {
    let definition = build_service_definition_from_registry_values(
        "console",
        &[sz("ImagePath", "/bin/sh"), sz("TTYPath", "/dev/tty1")],
    )
    .expect("definition");

    assert_eq!(definition.console_path.as_deref(), Some("/dev/tty1"));
}

#[test]
fn absent_tty_path_means_daemon_stdio() {
    let definition =
        build_service_definition_from_registry_values("app", &[sz("ImagePath", "/sbin/app")])
            .expect("definition");

    assert_eq!(definition.console_path, None);
}

/// An empty value clears the terminal rather than being rejected, so a seeded
/// or inherited TTYPath can be turned off without deleting the value.
#[test]
fn empty_tty_path_clears_the_terminal() {
    let definition = build_service_definition_from_registry_values(
        "app",
        &[sz("ImagePath", "/sbin/app"), sz("TTYPath", "")],
    )
    .expect("definition");

    assert_eq!(definition.console_path, None);
}

#[test]
fn relative_tty_path_is_rejected() {
    let error = build_service_definition_from_registry_values(
        "app",
        &[sz("ImagePath", "/sbin/app"), sz("TTYPath", "tty1")],
    )
    .expect_err("relative TTYPath must be rejected");

    assert!(format!("{error:?}").contains("TTYPath"), "{error:?}");
}

/// `tty:released` — the queue on a terminal. A service names the device in
/// `TTYPath` and asks to be started when whoever holds it lets go.
#[test]
fn tty_released_trigger_is_accepted() {
    let definition = build_service_definition_from_registry_values(
        "login-console",
        &[
            sz("ImagePath", "/bin/login"),
            sz("TTYPath", "/dev/console"),
            multi_sz("Triggers", &["tty:released"]),
        ],
    )
    .expect("definition");

    assert_eq!(definition.triggers, vec![ServiceTrigger::TtyReleased]);
    assert!(definition.has_tty_released_trigger());
    // Not a boot-plan member and not a deferred start either: the terminal
    // coming free is the only thing that starts it.
    assert!(!definition.has_boot_trigger());
    assert!(!definition.has_boot_settled_trigger());
}

/// `TTYPrecedence` decides who wins a terminal several services want at once.
#[test]
fn tty_precedence_is_accepted_alongside_a_tty_path() {
    let definition = build_service_definition_from_registry_values(
        "oobe",
        &[
            sz("ImagePath", "/sbin/oobe"),
            sz("TTYPath", "/dev/console"),
            dword("TTYPrecedence", 100),
        ],
    )
    .expect("definition");

    assert_eq!(definition.console_precedence, 100);
}

#[test]
fn absent_tty_precedence_is_the_bottom_of_the_queue() {
    let definition = build_service_definition_from_registry_values(
        "login-console",
        &[sz("ImagePath", "/bin/login"), sz("TTYPath", "/dev/console")],
    )
    .expect("definition");

    assert_eq!(definition.console_precedence, 0);
}

/// Both terminal fields are about a terminal the service names, and neither
/// means anything without one. Refused rather than ignored: a `tty:released`
/// with no `TTYPath` describes a service that would silently never start, and
/// starting is the entire point of the trigger.
#[test]
fn the_tty_fields_are_refused_without_a_tty_path() {
    let error = build_service_definition_from_registry_values(
        "login-console",
        &[
            sz("ImagePath", "/bin/login"),
            multi_sz("Triggers", &["tty:released"]),
        ],
    )
    .expect_err("tty:released without TTYPath must be rejected");
    assert!(
        format!("{error:?}").contains("FieldRequiresTtyPath"),
        "{error:?}"
    );

    let error = build_service_definition_from_registry_values(
        "app",
        &[sz("ImagePath", "/sbin/app"), dword("TTYPrecedence", 5)],
    )
    .expect_err("TTYPrecedence without TTYPath must be rejected");
    assert!(
        format!("{error:?}").contains("FieldRequiresTtyPath"),
        "{error:?}"
    );
}

/// An emptied `TTYPath` leaves no terminal, so it takes the fields that
/// depend on one down with it — the same rule, reached by the other route.
#[test]
fn an_emptied_tty_path_refuses_the_fields_that_depend_on_it() {
    let error = build_service_definition_from_registry_values(
        "login-console",
        &[
            sz("ImagePath", "/bin/login"),
            sz("TTYPath", ""),
            multi_sz("Triggers", &["tty:released"]),
        ],
    )
    .expect_err("an emptied TTYPath must take tty:released with it");

    assert!(
        format!("{error:?}").contains("FieldRequiresTtyPath"),
        "{error:?}"
    );
}

/// The `tty:` namespace carries the same arity rule as `boot:` — a bare word
/// that names no event is an error, and so is an unlisted sub-type.
#[test]
fn unknown_tty_sub_triggers_are_rejected() {
    for trigger in ["tty", "tty:", "tty:freed", "tty:release"] {
        let error = build_service_definition_from_registry_values(
            "app",
            &[
                sz("ImagePath", "/sbin/app"),
                sz("TTYPath", "/dev/console"),
                multi_sz("Triggers", &[trigger]),
            ],
        )
        .expect_err("{trigger} must be rejected");

        assert!(
            format!("{error:?}").contains("InvalidTrigger"),
            "{trigger}: {error:?}",
        );
    }
}

/// `boot:settled` — a boot trigger that waits for the boot set to stop moving.
#[test]
fn boot_settled_trigger_is_accepted() {
    let definition = build_service_definition_from_registry_values(
        "login-console",
        &[
            sz("ImagePath", "/bin/login"),
            multi_sz("Triggers", &["boot:settled"]),
        ],
    )
    .expect("definition");

    assert_eq!(definition.triggers, vec![ServiceTrigger::BootSettled]);
    // Deliberately NOT a boot-plan member: it starts after the plan, so
    // counting it would put it in the parallel-start budget and boot-success
    // accounting and let it block the boot it exists to stay out of.
    assert!(!definition.has_boot_trigger());
    assert!(definition.has_boot_settled_trigger());
}

/// Opening the `boot:` namespace kept its arity rule rather than dropping it:
/// a typo is still an error, not a silently-tolerated unknown trigger.
#[test]
fn unknown_boot_sub_triggers_are_still_rejected() {
    for trigger in ["boot:setled", "boot:anything", "boot:"] {
        build_service_definition_from_registry_values(
            "app",
            &[
                sz("ImagePath", "/sbin/app"),
                multi_sz("Triggers", &[trigger]),
            ],
        )
        .expect_err(trigger);
    }
}
