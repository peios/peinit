use crate::boot::BootMode;
use crate::boot::phase2::prepare_phase2_boot_plan;
use crate::execution::graph::{GraphContextId, GraphExecutionStore, ReadyGraphOperationAction};
use crate::execution::satisfaction::StartSatisfactionRequest;
use crate::execution::start::{
    StartExecutionOutcome, StartExecutionRequest, StartExecutionStore, StartReadyContext,
    begin_ready_start,
};
use crate::ids::{JobId, JobIdAllocator, OperationId, OperationIdAllocator};
use crate::job::{JobEvent, JobStore, ProcessHandle};
use crate::operation::store::OperationStore;
use crate::security::TokenSummary;
use crate::service::{ServiceDefinition, ServiceTable, ServiceType};

pub(crate) const OBSERVED_AT_NS: u64 = 1_000_000_000;
const STARTED_AT_NS: u64 = 1_000_001_000;
const SATISFIED_AT_NS: u64 = 1_000_002_000;
pub(crate) const LAUNCHED_AT_NS: u64 = 1_000_001_500;

pub(crate) struct StartedBootGraph {
    pub(crate) services: ServiceTable,
    pub(crate) operations: OperationStore,
    pub(crate) jobs: JobStore,
    pub(crate) graph: GraphExecutionStore,
    pub(crate) job_ids: JobIdAllocator,
    pub(crate) start: StartExecutionStore,
    pub(crate) context_id: GraphContextId,
    pub(crate) started_operation_id: OperationId,
    pub(crate) started_job_id: JobId,
}

impl StartedBootGraph {
    pub(crate) fn new(definitions: Vec<ServiceDefinition>, first_service: &str) -> Self {
        let mut operation_ids = OperationIdAllocator::new();
        let mut job_ids = JobIdAllocator::new();
        let plan = prepare_phase2_boot_plan(
            BootMode::Full,
            &definitions,
            10,
            OBSERVED_AT_NS,
            &mut operation_ids,
            &mut job_ids,
        )
        .expect("boot plan");
        let mut services =
            ServiceTable::from_boot_snapshot(definitions.clone()).expect("service table");
        let mut operations = OperationStore::new();
        operations
            .dispatch_phase2_boot_plan(&plan)
            .expect("operation dispatch");
        let mut graph = GraphExecutionStore::new();
        let context_id = graph
            .create_boot_context(&plan, &definitions)
            .expect("graph context");
        let ready = loop {
            let ready = graph.release_ready(context_id, 10).expect("ready start");
            let Some(ready) = ready.into_iter().next() else {
                panic!("requested ready start");
            };
            match ready.action {
                ReadyGraphOperationAction::PreStartCheck => {
                    graph
                        .apply_pre_start_check_passed(ready.operation_id)
                        .expect("pre-start check passed");
                }
                ReadyGraphOperationAction::Start if ready.service == first_service => break ready,
                ReadyGraphOperationAction::Start => {
                    panic!("unexpected ready service {}", ready.service);
                }
            }
        };
        let started_operation_id = ready.operation_id;
        let mut jobs = JobStore::new();
        let mut start = StartExecutionStore::new();
        let start_dispatch = begin_ready_start(
            &mut services,
            &mut operations,
            &mut graph,
            &mut jobs,
            &mut job_ids,
            &mut start,
            StartExecutionRequest {
                ready,
                resolved_identity: "SYSTEM".to_string(),
                token_summary: token_summary(),
                started_at_ns: STARTED_AT_NS,
            },
        )
        .expect("begin start");
        let StartExecutionOutcome::Job(start_dispatch) = start_dispatch else {
            panic!("expected created start job");
        };
        let started_job_id = start_dispatch.job_id;

        Self {
            services,
            operations,
            jobs,
            graph,
            job_ids,
            start,
            context_id,
            started_operation_id,
            started_job_id,
        }
    }

    pub(crate) fn start_ready_context(&mut self) -> StartReadyContext<'_> {
        StartReadyContext {
            services: &mut self.services,
            operations: &mut self.operations,
            graph: &mut self.graph,
            jobs: &mut self.jobs,
            job_ids: &mut self.job_ids,
            start_store: &mut self.start,
        }
    }

    pub(crate) fn mark_started_job_running(&mut self) -> JobEvent {
        self.jobs
            .start_job(
                self.started_job_id,
                ProcessHandle {
                    pid: 4242,
                    pidfd: 9,
                },
                LAUNCHED_AT_NS,
            )
            .expect("start job")
    }
}

pub(crate) fn service(name: &str, service_type: ServiceType) -> ServiceDefinition {
    let mut definition = ServiceDefinition::simple_system_boot(name, &format!("/sbin/{name}"));
    definition.service_type = service_type;
    definition
}

pub(crate) fn satisfaction_request(
    service: &str,
    operation_id: OperationId,
) -> StartSatisfactionRequest {
    StartSatisfactionRequest {
        service: service.to_string(),
        operation_id,
        satisfied_at_ns: SATISFIED_AT_NS,
        result: "ready".to_string(),
    }
}

fn token_summary() -> TokenSummary {
    TokenSummary::requested_identity("SYSTEM")
}
