use crate::control::reload_config::reload_config;
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::logging::DEFAULT_PRE_EVENTD_BUFFER_BYTES;
use crate::logging::RuntimeLogConfig;
use crate::registry::{RegistryConfigWarning, SUPPORTED_SERVICES_SCHEMA_VERSION};
use crate::service::{Readiness, ServiceGraphWarning, ServiceTable};

use super::{StaticRegistry, service, service_table};

#[test]
fn applies_valid_registry_snapshot_to_service_table() {
    let mut services = service_table(&["old", "app"]);
    let mut app = service("app", "/sbin/app-v2");
    app.requires.push("db".to_string());
    let db = service("db", "/sbin/db");
    let mut registry = StaticRegistry::services(vec![app, db]);

    let outcome = reload_config(&mut registry, &mut services).expect("reload config");

    assert_eq!(registry.reads, 1);
    assert_eq!(outcome.summary.added, vec!["db"]);
    assert_eq!(outcome.summary.updated, vec!["app"]);
    assert_eq!(outcome.summary.discarded, vec!["old"]);
    assert!(outcome.warnings.is_empty());
    assert!(outcome.config_warnings.is_empty());
    assert!(services.get("old").is_none());
    assert_eq!(
        services.definition("app").expect("app").image_path,
        "/sbin/app-v2",
    );
}

#[test]
fn reloads_schema_guard_control_security_and_limits() {
    let mut services = service_table(&["app"]);
    let registry_limits = ControlSocketLimits {
        max_connections: 9,
        max_request_bytes: 16_384,
        connection_timeout_secs: 11,
    };
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .schema_version(SUPPORTED_SERVICES_SCHEMA_VERSION + 1)
        .control_security(ControlSecurityDescriptor::RegistryBinary(vec![7, 8]))
        .control_limits(registry_limits)
        .log_config(12_000, 128_000)
        .shutdown_timeout_secs(23);

    let outcome = reload_config(&mut registry, &mut services).expect("reload config");

    assert_eq!(
        outcome.services_schema_version,
        SUPPORTED_SERVICES_SCHEMA_VERSION + 1
    );
    assert_eq!(
        outcome.config_warnings,
        vec![RegistryConfigWarning::NewerServicesSchemaVersion {
            observed: SUPPORTED_SERVICES_SCHEMA_VERSION + 1,
            supported: SUPPORTED_SERVICES_SCHEMA_VERSION,
        }]
    );
    assert_eq!(
        outcome.control_security,
        ControlSecurityDescriptor::RegistryBinary(vec![7, 8]),
    );
    assert_eq!(outcome.control_limits, registry_limits);
    assert_eq!(outcome.log_config.max_line_bytes, 12_000);
    assert_eq!(outcome.log_config.max_buffer_per_service_bytes, 128_000);
    assert_eq!(
        outcome.log_config.pre_eventd_buffer_bytes,
        DEFAULT_PRE_EVENTD_BUFFER_BYTES,
    );
    assert_eq!(outcome.shutdown_settings.global_timeout_secs, 23);
}

#[test]
fn reloads_eventd_log_socket_path() {
    let mut services = service_table(&["app"]);
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .eventd_log_socket_path("/run/services/peinit/eventd.sock");

    let outcome = reload_config(&mut registry, &mut services).expect("reload config");

    assert_eq!(
        outcome.eventd_log_socket_path.as_deref(),
        Some("/run/services/peinit/eventd.sock"),
    );
}

#[test]
fn returns_nonfatal_validation_warnings() {
    let mut app = service("app", "/sbin/app");
    app.binds_to.push("db".to_string());
    let mut db = service("db", "/sbin/db");
    db.readiness = Readiness::Alive;
    let mut registry = StaticRegistry::services(vec![app, db]);
    let mut services = service_table(&[]);

    let outcome = reload_config(&mut registry, &mut services).expect("reload config");

    assert_eq!(
        outcome.warnings,
        vec![ServiceGraphWarning::AliveReadinessWithHardDependents {
            service: "db".to_string(),
            dependents: vec!["app".to_string()],
        }]
    );
    assert_eq!(services.service_names(), vec!["app", "db"]);
}

/// reload-config must read all four log knobs, not two.
///
/// It used to read only `MaxLogLineLength` and `MaxLogBufferPerService`,
/// starting from `RuntimeLogConfig::default()` — so every reload silently
/// reverted `LogReadBytesPerEvent` and `PreEventdBuffer` to their compiled-in
/// defaults whatever the registry said. Once PEI-356 made the pre-eventd value
/// actually resize the buffer, that revert became an active shrink on every
/// reload.
#[test]
fn reload_reads_every_log_knob_not_just_the_two_it_used_to() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .with_max_log_line_length(Some(12_000))
        .with_max_log_buffer_per_service(Some(128_000))
        .with_log_read_bytes_per_event(Some(4_096))
        .with_pre_eventd_buffer_bytes(Some(2_097_152));
    let mut services = ServiceTable::default();

    let outcome = reload_config(&mut registry, &mut services).expect("reload");

    assert_eq!(outcome.log_config.max_line_bytes, 12_000);
    assert_eq!(outcome.log_config.max_buffer_per_service_bytes, 128_000);
    assert_eq!(outcome.log_config.read_bytes_per_event, 4_096);
    assert_eq!(outcome.log_config.pre_eventd_buffer_bytes, 2_097_152);
    assert_eq!(
        outcome.log_config.eventd_log_datagram_bytes,
        crate::logging::DEFAULT_EVENTD_LOG_DATAGRAM_BYTES
    );
    assert!(
        outcome.config_warnings.is_empty(),
        "{:?}",
        outcome.config_warnings
    );
}

/// The same below-minimum rule applies on the reload path as at boot, and the
/// warning reaches the caller through the reload outcome.
#[test]
fn reload_rejects_a_below_minimum_log_knob_and_warns() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .with_pre_eventd_buffer_bytes(Some(0));
    let mut services = ServiceTable::default();

    let outcome = reload_config(&mut registry, &mut services).expect("reload");

    assert_eq!(
        outcome.log_config.pre_eventd_buffer_bytes,
        RuntimeLogConfig::default().pre_eventd_buffer_bytes,
    );
    assert!(
        outcome
            .warning_messages()
            .iter()
            .any(|message| message.contains("PreEventdBuffer")),
        "expected the warning to reach the caller, got {:?}",
        outcome.warning_messages(),
    );
}
