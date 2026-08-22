use crate::boot::phase2::run_phase2_boot;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::operation::store::OperationStore;

use crate::logging::RuntimeLogConfig;
use crate::registry::RegistryConfigWarning;

use super::{FixedClock, OBSERVED_AT_NS, StaticRegistry, service, settings};

#[test]
fn run_phase2_boot_reads_snapshot_plans_and_dispatches_operations_atomically() {
    let mut app = service("app", "/sbin/app");
    app.requires.push("authd".to_string());
    let mut authd = service("authd", "/sbin/authd");
    authd.triggers.clear();
    let mut registry = StaticRegistry::services(vec![app, authd]);
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let mut operations = OperationStore::new();

    let run = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect("phase2 boot");

    assert_eq!(registry.reads, 1);
    assert_eq!(clock.reads, 1);
    assert_eq!(operation_ids.next_sequence(), 2);
    assert_eq!(job_ids.next_sequence(), 2);
    assert_eq!(
        run.plan
            .starts
            .iter()
            .map(|start| start.service.as_str())
            .collect::<Vec<_>>(),
        vec!["authd", "app"],
    );
    assert_eq!(
        run.dispatch.start_operation_ids,
        run.plan
            .starts
            .iter()
            .map(|start| start.operation_id)
            .collect::<Vec<_>>(),
    );
    assert_eq!(run.service_table.service_names(), vec!["app", "authd"]);
    assert_eq!(
        run.service_table
            .definition("app")
            .expect("app definition")
            .image_path,
        "/sbin/app",
    );
    assert_eq!(
        operations.active_for_service("authd"),
        vec![run.plan.starts[0].operation_id],
    );
    assert_eq!(
        operations.active_for_service("app"),
        vec![run.plan.starts[1].operation_id],
    );
}

/// The boot set is exactly what the registry defines. console, authd, lpsd and
/// login were once appended here from compiled-in definitions when
/// `peios.console=1` / `peios.login=1` were on the command line; they are
/// ordinary registry services now, so nothing is added to what the registry
/// returned.
#[test]
fn the_boot_set_is_exactly_the_registry_service_table() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")]);
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let mut operations = OperationStore::new();

    let run = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect("phase2 boot");

    assert_eq!(run.service_table.service_names(), vec!["app"]);
    for injected in ["console", "authd", "lpsd", "login"] {
        assert!(
            !run.service_table.service_names().contains(&injected),
            "{injected} must come from the registry, not from peinit",
        );
    }
}

#[test]
fn registry_boot_settings_override_phase2_defaults() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .with_max_parallel_starts(Ok(Some(4)))
        .with_boot_success_grace_secs(Ok(Some(12)))
        .with_max_log_line_length(Ok(Some(12_000)))
        .with_max_log_buffer_per_service(Ok(Some(128_000)))
        .with_shutdown_timeout_secs(Ok(Some(17)))
        .with_post_kill_timeout_secs(Ok(Some(9)))
        .with_log_read_bytes_per_event(Ok(Some(4_096)))
        .with_pre_eventd_buffer_bytes(Ok(Some(2_097_152)));
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let mut operations = OperationStore::new();

    let run = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect("phase2 boot");

    assert_eq!(run.plan.max_parallel_starts, 4);
    assert_eq!(run.settings.max_parallel_starts, 4);
    assert_eq!(run.settings.boot_success_grace_secs, 12);
    assert_eq!(run.log_config.max_line_bytes, 12_000);
    assert_eq!(run.log_config.max_buffer_per_service_bytes, 128_000);
    assert_eq!(run.shutdown_settings.global_timeout_secs, 17);
    assert_eq!(run.shutdown_settings.post_kill_timeout_secs, 9);
    assert_eq!(run.log_config.read_bytes_per_event, 4_096);
    assert_eq!(run.log_config.pre_eventd_buffer_bytes, 2_097_152);
}

/// Each of these is a field whose struct siblings were already registry-backed
/// while it was not, so the gap was an omission rather than a decision. Absent
/// values must still fall through to the compiled-in defaults.
#[test]
fn newly_configurable_settings_fall_back_to_their_defaults() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")]);
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let mut operations = OperationStore::new();

    let run = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect("phase2 boot");

    let shutdown_defaults = crate::shutdown::ShutdownSettings::default();
    let log_defaults = crate::logging::RuntimeLogConfig::default();
    assert_eq!(
        run.shutdown_settings.post_kill_timeout_secs,
        shutdown_defaults.post_kill_timeout_secs,
    );
    assert_eq!(
        run.log_config.read_bytes_per_event,
        log_defaults.read_bytes_per_event,
    );
    assert_eq!(
        run.log_config.pre_eventd_buffer_bytes,
        log_defaults.pre_eventd_buffer_bytes,
    );
}

/// A log knob below its minimum keeps the compiled-in default and reports a
/// warning. Failing the boot over a logging typo is the outcome peinit's own
/// command-line parser explicitly rejects; honouring it silently is the bug.
#[test]
fn a_log_knob_below_its_minimum_keeps_the_default_and_warns() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .with_pre_eventd_buffer_bytes(Ok(Some(0)));
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let mut store = OperationStore::new();

    let run = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operations,
        &mut jobs,
        &mut store,
    )
    .expect("boot run");

    assert_eq!(
        run.log_config.pre_eventd_buffer_bytes,
        RuntimeLogConfig::default().pre_eventd_buffer_bytes,
    );
    assert!(
        run.config_warnings.iter().any(|warning| matches!(
            warning,
            RegistryConfigWarning::LogConfigValueBelowMinimum {
                key: "PreEventdBuffer",
                configured: 0,
                ..
            }
        )),
        "expected a below-minimum warning, got {:?}",
        run.config_warnings,
    );
}

/// A value at the minimum is accepted — the boundary is inclusive, so the
/// documented minimum is a usable value rather than one below the first usable
/// one.
#[test]
fn a_log_knob_exactly_at_its_minimum_is_accepted() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .with_pre_eventd_buffer_bytes(Ok(Some(4096)));
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    let mut store = OperationStore::new();

    let run = run_phase2_boot(
        settings(),
        &mut registry,
        &mut clock,
        &mut operations,
        &mut jobs,
        &mut store,
    )
    .expect("boot run");

    assert_eq!(run.log_config.pre_eventd_buffer_bytes, 4096);
    assert!(run.config_warnings.is_empty(), "{:?}", run.config_warnings);
}
