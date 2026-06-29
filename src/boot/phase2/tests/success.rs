use crate::service::ServiceDefinition;

use super::super::StartCause;
use super::plan;

#[test]
fn allocates_operation_and_job_ids_internally_for_boot_graph() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.requires.push("authd".to_string());
    let mut authd = ServiceDefinition::simple_system_boot("authd", "/sbin/authd");
    authd.triggers.clear();

    let boot = plan(&[app, authd]);

    assert_eq!(boot.starts.len(), 2);
    assert_eq!(boot.starts[0].service, "authd");
    assert_eq!(boot.starts[0].cause, StartCause::DependencyStart);
    assert_eq!(boot.starts[1].service, "app");
    assert_eq!(boot.starts[1].cause, StartCause::ExplicitStart);
    assert_ne!(boot.starts[0].operation_id, boot.starts[1].operation_id);
    assert_ne!(boot.starts[0].job_id, boot.starts[1].job_id);
}

#[test]
fn missing_wants_are_ignored() {
    let mut app = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    app.wants.push("optional-metrics".to_string());

    let boot = plan(&[app]);

    assert_eq!(boot.starts.len(), 1);
    assert_eq!(boot.starts[0].service, "app");
    assert!(boot.blocked.is_empty());
}
