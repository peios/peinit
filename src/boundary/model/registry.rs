use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::jobs::socket::JobsSocketLimits;
use crate::provisioning::ProvisionedPathRegistrySnapshot;
use crate::registry::SUPPORTED_SERVICES_SCHEMA_VERSION;
use crate::service::{ServiceDefinition, ServiceEnvironmentVariable};
use crate::timer::state::TimerLastRunStorage;

use super::error::BoundaryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryWatchRoot {
    Services,
    Init,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryWatchEventKind {
    ValueSet,
    ValueDeleted,
    SubkeyCreated,
    SubkeyDeleted,
    SecurityDescriptorChanged,
    KeyDeleted,
    Overflow,
    Other(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryWatchEvent {
    pub root: RegistryWatchRoot,
    pub kind: RegistryWatchEventKind,
    pub name: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerLastRunWriteRequest {
    pub service: String,
    pub schedule: String,
    pub storage: TimerLastRunStorage,
    pub timestamp_realtime_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerLastRunWriteOutcome {
    Queued,
}

/// A service key that exists but whose values will not decode.
///
/// Carried by name rather than as a `ServiceDefinition`, because there is no
/// valid definition to carry — which is the whole problem. `BlockedReason`
/// keys on the name alone, so this is enough to mark the service Failed with
/// `TransitionCause::ValidationError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndecodableService {
    pub name: String,
    pub message: String,
}

/// The result of a read that tolerates individual bad definitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServiceDefinitionsRead {
    pub definitions: Vec<ServiceDefinition>,
    pub undecodable: Vec<UndecodableService>,
}

pub trait RegistryClient {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError>;

    /// Read definitions, reporting undecodable keys instead of failing.
    ///
    /// Boot uses this; reload-config deliberately does not. Reload is atomic
    /// and has somewhere to fall back to — the configuration already running —
    /// so refusing the whole transaction is right there. Boot has no such
    /// fallback, which is why the same all-or-nothing policy has the opposite
    /// consequence: one typo in one service key took the machine to the
    /// recovery console.
    ///
    /// The default is the conservative one, so an implementation that has not
    /// thought about partial reads keeps its existing behaviour.
    fn read_service_definitions_partial(
        &mut self,
    ) -> Result<ServiceDefinitionsRead, BoundaryError> {
        Ok(ServiceDefinitionsRead {
            definitions: self.read_service_definitions()?,
            undecodable: Vec::new(),
        })
    }

    fn read_max_parallel_starts(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_boot_success_grace_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_shutdown_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_post_kill_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_settle_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_log_read_bytes_per_event(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_pre_eventd_buffer_bytes(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_max_log_line_length(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_max_log_buffer_per_service(&mut self) -> Result<Option<u32>, BoundaryError> {
        Ok(None)
    }

    fn read_global_environment(
        &mut self,
    ) -> Result<Vec<ServiceEnvironmentVariable>, BoundaryError> {
        Ok(Vec::new())
    }

    fn read_provisioned_paths(&mut self) -> Result<ProvisionedPathRegistrySnapshot, BoundaryError> {
        Ok(ProvisionedPathRegistrySnapshot::empty())
    }

    fn read_eventd_log_socket_path(&mut self) -> Result<Option<String>, BoundaryError> {
        Ok(None)
    }

    fn read_services_schema_version(&mut self) -> Result<u32, BoundaryError> {
        Ok(SUPPORTED_SERVICES_SCHEMA_VERSION)
    }

    /// Ensure the base service-registry structure (`Machine\System\Services` and
    /// its `SchemaVersion`) exists, creating it on a fresh system. Default
    /// no-op; the real LCS client provisions it.
    fn provision_base_registry(&mut self) -> Result<(), BoundaryError> {
        Ok(())
    }

    fn read_control_security(&mut self) -> Result<ControlSecurityDescriptor, BoundaryError> {
        Ok(ControlSecurityDescriptor::Default)
    }

    fn read_control_socket_limits(&mut self) -> Result<ControlSocketLimits, BoundaryError> {
        Ok(ControlSocketLimits::default())
    }

    fn read_jobs_socket_limits(&mut self) -> Result<JobsSocketLimits, BoundaryError> {
        Ok(JobsSocketLimits::default())
    }

    fn read_timer_last_run(
        &mut self,
        _service: &str,
        _schedule: &str,
        _storage: TimerLastRunStorage,
    ) -> Result<Option<u64>, BoundaryError> {
        Err(BoundaryError::Registry(
            "timer last-run reads are not supported by this registry client".to_string(),
        ))
    }
}

pub trait TimerLastRunWriter {
    fn queue_timer_last_run_write(
        &mut self,
        _request: TimerLastRunWriteRequest,
    ) -> Result<TimerLastRunWriteOutcome, BoundaryError> {
        Err(BoundaryError::Registry(
            "timer last-run writes are not supported by this registry client".to_string(),
        ))
    }
}

pub trait RegistryWatchSource {
    fn drain_registry_watch_events(
        &mut self,
        fd: i32,
    ) -> Result<Vec<RegistryWatchEvent>, BoundaryError>;
}
