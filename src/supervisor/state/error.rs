use crate::boot::phase2::Phase2BootRunError;
use crate::boundary::BoundaryError;
use crate::control::lifecycle::LifecycleCommandError;
use crate::execution::control::ControlExecutionError;
use crate::execution::graph::{GraphContextBuildError, GraphExecutionError};
use crate::execution::job_started::ServiceMainJobStartedError;
use crate::execution::job_terminal::ServiceMainJobTerminalError;
use crate::execution::launch::LaunchCreatedJobError;
use crate::execution::notify::NotifyApplyError;
use crate::execution::restart_policy::RestartPolicyRelaunchError;
use crate::execution::start::StartExecutionError;
use crate::ids::IdAllocationError;
use crate::job::JobStoreError;
use crate::notify::NotifyParseError;
use crate::shutdown::ShutdownError;
use crate::supervisor::health::HealthCheckError;
use crate::supervisor::notify::timeout_extension::TimeoutExtensionError;
use crate::supervisor::watchdog::WatchdogError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorError {
    Phase2Boot(Phase2BootRunError),
    GraphContext(GraphContextBuildError),
    Graph(GraphExecutionError),
    MissingStartCredentials { service: String },
    RequestIdAllocation(IdAllocationError),
    Start(StartExecutionError),
    Lifecycle(LifecycleCommandError),
    Clock(BoundaryError),
    Launch(LaunchCreatedJobError),
    JobStore(JobStoreError),
    JobStarted(ServiceMainJobStartedError),
    JobTerminal(ServiceMainJobTerminalError),
    RestartPolicy(RestartPolicyRelaunchError),
    Control(ControlExecutionError),
    NotifyParse(NotifyParseError),
    Notify(NotifyApplyError),
    Health(HealthCheckError),
    Watchdog(WatchdogError),
    TimeoutExtension(TimeoutExtensionError),
    ProcessControl(BoundaryError),
    Shutdown(ShutdownError),
    Timer(BoundaryError),
    FilesystemCheck(BoundaryError),
}
