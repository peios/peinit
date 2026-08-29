use crate::runtime::RuntimeJobsChannel;
use crate::runtime::{RuntimeControlLimits, RuntimeEventRegistrar};
use crate::supervisor::Supervisor;

use super::support::deadline_wait_timeout_ms;
use crate::runtime::linux::LinuxShutdownRuntime;

impl LinuxShutdownRuntime {
    pub(super) fn sync_reloadable_config(&mut self, supervisor: &Supervisor) {
        let control_limits = supervisor.control_limits();
        self.control_connections
            .set_max_connections(control_limits.max_connections);
        self.config.max_control_connections = control_limits.max_connections;
        self.config.control_limits.max_request_bytes = control_limits.max_request_bytes;
        self.config.control_limits.connection_timeout_secs = control_limits.connection_timeout_secs;
        self.config.control_security = supervisor.control_security().clone();
        self.log_pipes
            .update_config(supervisor.log_config().clone());
        let jobs_limits = supervisor.jobs_limits();
        self.jobs_channel
            .set_max_jobs_connections(jobs_limits.max_connections);
        self.config.max_jobs_connections = jobs_limits.max_connections;
        self.jobs_connection_timeout_secs = jobs_limits.connection_timeout_secs;
    }

    pub(super) fn runtime_control_limits(&self, supervisor: &Supervisor) -> RuntimeControlLimits {
        RuntimeControlLimits::new(
            self.config.control_limits.max_read_bytes,
            supervisor.control_limits().max_request_bytes,
            supervisor.control_limits().connection_timeout_secs,
        )
    }

    pub(super) fn close_idle_control_connections(&mut self, now_ns: u64) -> Vec<i32> {
        let timeout_secs = self.config.control_limits.connection_timeout_secs;
        let fds = self.control_connections.idle_fds(now_ns, timeout_secs);
        for fd in &fds {
            let _ = self.epoll.unregister_source(*fd);
            self.control_connections.remove(*fd);
        }
        fds
    }

    pub(super) fn close_idle_jobs_connections(&mut self, now_ns: u64) -> Vec<i32> {
        let timeout_secs = self.jobs_connection_timeout_secs;
        self.jobs_channel
            .close_idle_jobs_connections(&mut self.epoll, now_ns, timeout_secs)
    }

    pub(super) fn runtime_wait_timeout_ms(&self, supervisor: &Supervisor, now_ns: u64) -> i32 {
        deadline_wait_timeout_ms(
            [
                self.next_control_idle_deadline_ns(),
                self.jobs_channel
                    .next_jobs_idle_deadline_ns(self.jobs_connection_timeout_secs),
                supervisor.next_operation_maintenance_deadline_ns(),
            ]
            .into_iter()
            .flatten()
            .min(),
            now_ns,
        )
    }

    fn next_control_idle_deadline_ns(&self) -> Option<u64> {
        let timeout_secs = self.config.control_limits.connection_timeout_secs;
        self.control_connections.next_idle_deadline_ns(timeout_secs)
    }
}
