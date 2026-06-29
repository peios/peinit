use crate::execution::job_terminal::apply_service_main_job_terminal;
use crate::execution::restart_policy::{
    RestartPolicyRelaunchContext, RestartPolicyRelaunchRequest, begin_due_restart_policy_relaunch,
};
use crate::execution::satisfaction::{StartSatisfactionRequest, apply_start_satisfaction};
use crate::execution::start::StartExecutionStore;
use crate::execution::test_support::{
    OBSERVED_AT_NS, StartedBootGraph, satisfaction_request, service,
};
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::job::JobState;
use crate::operation::{OperationSource, OperationState};
use crate::service::ServiceType;
use crate::service::runtime::{ServiceState, TransitionCause};

const SATISFIED_AT_NS: u64 = 1_000_002_000;
const ENDED_AT_NS: u64 = 2_000_000_000;
const RESTART_AT_NS: u64 = 3_000_000_000;

#[test]
fn crashed_active_service_relaunches_after_restart_backoff() {
    let app = service("app", ServiceType::Simple);
    let mut fixture = StartedBootGraph::new(vec![app], "app");
    fixture.mark_started_job_running();
    satisfy_start(&mut fixture);
    let terminal_event = fixture
        .jobs
        .complete_job(fixture.started_job_id, ENDED_AT_NS, 1)
        .expect("complete crashed job");

    apply_service_main_job_terminal(&mut fixture.start_ready_context(), terminal_event)
        .expect("apply terminal job");
    let backoff_runtime = fixture.services.runtime("app").expect("backoff runtime");
    assert_eq!(backoff_runtime.state, ServiceState::Backoff);
    assert_eq!(
        backoff_runtime.restart_backoff_until_ns,
        Some(RESTART_AT_NS)
    );

    let mut operation_ids = operation_ids_after_boot_start();
    let mut job_ids = job_ids_after_boot_start();
    let mut start_store = StartExecutionStore::new();
    let expected_restart_operation = operation_ids
        .clone()
        .allocate_batch(1, RESTART_AT_NS)
        .expect("expected restart operation")[0];
    let expected_restart_job = job_ids
        .clone()
        .allocate_batch(1, RESTART_AT_NS)
        .expect("expected restart job")[0];

    let dispatch = begin_due_restart_policy_relaunch(
        &mut RestartPolicyRelaunchContext {
            services: &mut fixture.services,
            operations: &mut fixture.operations,
            graph: &mut fixture.graph,
            jobs: &mut fixture.jobs,
            start_store: &mut start_store,
            operation_ids: &mut operation_ids,
            job_ids: &mut job_ids,
        },
        RestartPolicyRelaunchRequest {
            service: "app".to_string(),
            observed_at_ns: RESTART_AT_NS,
            max_parallel_starts: 10,
        },
    )
    .expect("restart relaunch");

    assert_eq!(dispatch.context_id.as_u64(), 1);
    assert_eq!(dispatch.start_dispatches.len(), 1);
    assert_eq!(
        dispatch.admission.requested_operation.returned_operation_id,
        expected_restart_operation,
    );
    let start = &dispatch.start_dispatches[0];
    assert_eq!(start.job_id, expected_restart_job);
    assert_eq!(start.ready.service, "app");
    assert_eq!(start.ready.transition_cause, TransitionCause::RestartPolicy);

    let runtime = fixture.services.runtime("app").expect("runtime");
    assert_eq!(runtime.state, ServiceState::Starting);
    assert_eq!(runtime.cause, Some(TransitionCause::RestartPolicy));
    assert_eq!(runtime.generation, 2);
    assert_eq!(runtime.restart_backoff_until_ns, None);
    assert_eq!(
        fixture
            .operations
            .get(expected_restart_operation)
            .expect("restart operation")
            .source,
        OperationSource::RestartPolicy,
    );
    assert_eq!(
        fixture
            .operations
            .get(expected_restart_operation)
            .expect("restart operation")
            .state,
        OperationState::Running,
    );
    let job = fixture.jobs.get(expected_restart_job).expect("restart job");
    assert_eq!(job.state, JobState::Created);
    assert_eq!(job.operation_id, Some(expected_restart_operation));
    assert_eq!(job.activation_generation, 2);
    assert_eq!(job.cgroup_generation, 0);
    assert_eq!(job.cgroup_id, "/sys/fs/cgroup/peinit/app/main");
    assert_eq!(job.resolved_identity, "SYSTEM");
    assert_eq!(
        fixture.jobs.active_for_service("app"),
        vec![expected_restart_job]
    );
    assert_eq!(operation_ids.next_sequence(), 2);
    assert_eq!(job_ids.next_sequence(), 2);
}

fn satisfy_start(fixture: &mut StartedBootGraph) {
    let request = satisfaction_request("app", fixture.started_operation_id);
    apply_start_satisfaction(
        &mut fixture.services,
        &mut fixture.operations,
        &mut fixture.graph,
        StartSatisfactionRequest {
            satisfied_at_ns: SATISFIED_AT_NS,
            ..request
        },
    )
    .expect("satisfy start");
}

fn operation_ids_after_boot_start() -> OperationIdAllocator {
    let mut operation_ids = OperationIdAllocator::new();
    operation_ids
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("skip boot operation id");
    operation_ids
}

fn job_ids_after_boot_start() -> JobIdAllocator {
    let mut job_ids = JobIdAllocator::new();
    job_ids
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("skip boot job id");
    job_ids
}
