use crate::boot::phase2::run_phase2_boot;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::operation::store::OperationStore;

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

#[test]
fn spawn_console_injects_the_compiled_in_console_service() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")]);
    let mut clock = FixedClock::at(OBSERVED_AT_NS);
    let mut operation_ids = OperationIdAllocator::new();
    let mut job_ids = JobIdAllocator::new();
    let mut operations = OperationStore::new();

    let mut settings = settings();
    settings.spawn_console = true;

    let run = run_phase2_boot(
        settings,
        &mut registry,
        &mut clock,
        &mut operation_ids,
        &mut job_ids,
        &mut operations,
    )
    .expect("phase2 boot");

    // The console service is not a registry entry — it is appended to the boot
    // set, so it appears alongside the registry-defined app.
    assert!(
        run.service_table
            .service_names()
            .contains(&crate::service::ServiceDefinition::CONSOLE_NAME),
    );
    let console = run
        .service_table
        .definition(crate::service::ServiceDefinition::CONSOLE_NAME)
        .expect("console definition");
    assert!(console.attach_console);
    assert_eq!(
        console.image_path,
        crate::service::ServiceDefinition::CONSOLE_IMAGE_PATH,
    );
}

#[test]
fn console_is_absent_without_spawn_console() {
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

    assert!(
        !run.service_table
            .service_names()
            .contains(&crate::service::ServiceDefinition::CONSOLE_NAME),
    );
}

#[test]
fn registry_boot_settings_override_phase2_defaults() {
    let mut registry = StaticRegistry::services(vec![service("app", "/sbin/app")])
        .with_max_parallel_starts(Ok(Some(4)))
        .with_boot_success_grace_secs(Ok(Some(12)))
        .with_max_log_line_length(Ok(Some(12_000)))
        .with_max_log_buffer_per_service(Ok(Some(128_000)))
        .with_shutdown_timeout_secs(Ok(Some(17)));
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
}
