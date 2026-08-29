use crate::boundary::{
    ChildReaper, FilesystemCheckHelperReader, RegistryClient, TimerLastRunWriter,
};
use crate::control::connection::{
    ControlConnectionIo, ControlConnectionRecord, ControlConnectionTable, ControlListener,
};
use crate::runtime::{
    RuntimeJobsChannel, RuntimeLifecycleDeadlineTimer, RuntimeNotifySource,
    RuntimePid1SignalSource, RuntimePowerButtonSource, RuntimeServiceLogPipes,
    RuntimeShutdownDeadlineTimer,
};

pub struct RuntimeShutdownEventSources<'a, I, L, S, H, N, D, T>
where
    I: ControlConnectionIo,
    L: ControlListener<Connection = I> + ?Sized,
    S: RuntimePid1SignalSource + ?Sized,
    H: ChildReaper + ?Sized,
    N: RuntimeNotifySource + ?Sized,
    D: RuntimeShutdownDeadlineTimer + ?Sized,
    T: RuntimeLifecycleDeadlineTimer + ?Sized,
{
    pub signal_source: &'a mut S,
    pub child_reaper: &'a mut H,
    pub notify_source: &'a mut N,
    pub control_listener: &'a mut L,
    pub control_connections: &'a mut ControlConnectionTable<ControlConnectionRecord<I>>,
    pub deadline_timer: &'a mut D,
    pub lifecycle_timer: &'a mut T,
    pub power_button_source: &'a mut dyn RuntimePowerButtonSource,
    pub filesystem_check_reader: &'a mut dyn FilesystemCheckHelperReader,
    pub log_pipes: &'a mut RuntimeServiceLogPipes,
    pub jobs_channel: &'a mut dyn RuntimeJobsChannel,
}

pub(crate) struct NoRuntimeRegistryClient;

impl RegistryClient for NoRuntimeRegistryClient {
    fn read_service_definitions(
        &mut self,
    ) -> Result<Vec<crate::service::ServiceDefinition>, crate::boundary::BoundaryError> {
        Err(crate::boundary::BoundaryError::Registry(
            "runtime registry client unavailable".to_string(),
        ))
    }
}

impl TimerLastRunWriter for NoRuntimeRegistryClient {}
