mod atomicity;
mod recovery;
mod success;

use crate::boot::BootMode;
use crate::boot::phase2::{DEFAULT_MAX_PARALLEL_STARTS, Phase2BootSettings};
use crate::boundary::{BoundaryError, Clock, RegistryClient};
use crate::ids::{OperationId, OperationIdAllocator};
use crate::service::ServiceDefinition;

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

#[derive(Debug, Clone)]
struct StaticRegistry {
    result: Result<Vec<ServiceDefinition>, BoundaryError>,
    max_parallel_starts: Option<Result<Option<u32>, BoundaryError>>,
    boot_success_grace_secs: Option<Result<Option<u32>, BoundaryError>>,
    shutdown_timeout_secs: Option<Result<Option<u32>, BoundaryError>>,
    max_log_line_length: Option<Result<Option<u32>, BoundaryError>>,
    max_log_buffer_per_service: Option<Result<Option<u32>, BoundaryError>>,
    post_kill_timeout_secs: Option<Result<Option<u32>, BoundaryError>>,
    log_read_bytes_per_event: Option<Result<Option<u32>, BoundaryError>>,
    pre_eventd_buffer_bytes: Option<Result<Option<u32>, BoundaryError>>,
    eventd_log_datagram_bytes: Option<Result<Option<u32>, BoundaryError>>,
    reads: usize,
}

impl StaticRegistry {
    fn services(services: Vec<ServiceDefinition>) -> Self {
        Self {
            result: Ok(services),
            max_parallel_starts: None,
            boot_success_grace_secs: None,
            shutdown_timeout_secs: None,
            max_log_line_length: None,
            max_log_buffer_per_service: None,
            post_kill_timeout_secs: None,
            log_read_bytes_per_event: None,
            pre_eventd_buffer_bytes: None,
            eventd_log_datagram_bytes: None,
            reads: 0,
        }
    }

    fn error(error: BoundaryError) -> Self {
        Self {
            result: Err(error),
            max_parallel_starts: None,
            boot_success_grace_secs: None,
            shutdown_timeout_secs: None,
            max_log_line_length: None,
            max_log_buffer_per_service: None,
            post_kill_timeout_secs: None,
            log_read_bytes_per_event: None,
            pre_eventd_buffer_bytes: None,
            eventd_log_datagram_bytes: None,
            reads: 0,
        }
    }

    fn with_max_parallel_starts(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.max_parallel_starts = Some(value);
        self
    }

    fn with_boot_success_grace_secs(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.boot_success_grace_secs = Some(value);
        self
    }

    fn with_shutdown_timeout_secs(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.shutdown_timeout_secs = Some(value);
        self
    }

    fn with_post_kill_timeout_secs(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.post_kill_timeout_secs = Some(value);
        self
    }

    fn with_log_read_bytes_per_event(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.log_read_bytes_per_event = Some(value);
        self
    }

    fn with_pre_eventd_buffer_bytes(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.pre_eventd_buffer_bytes = Some(value);
        self
    }

    fn with_eventd_log_datagram_bytes(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.eventd_log_datagram_bytes = Some(value);
        self
    }

    fn with_max_log_line_length(mut self, value: Result<Option<u32>, BoundaryError>) -> Self {
        self.max_log_line_length = Some(value);
        self
    }

    fn with_max_log_buffer_per_service(
        mut self,
        value: Result<Option<u32>, BoundaryError>,
    ) -> Self {
        self.max_log_buffer_per_service = Some(value);
        self
    }
}

impl RegistryClient for StaticRegistry {
    fn read_service_definitions(&mut self) -> Result<Vec<ServiceDefinition>, BoundaryError> {
        self.reads += 1;
        self.result.clone()
    }

    fn read_max_parallel_starts(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.max_parallel_starts.clone().unwrap_or(Ok(None))
    }

    fn read_boot_success_grace_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.boot_success_grace_secs.clone().unwrap_or(Ok(None))
    }

    fn read_shutdown_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.shutdown_timeout_secs.clone().unwrap_or(Ok(None))
    }

    fn read_post_kill_timeout_secs(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.post_kill_timeout_secs.clone().unwrap_or(Ok(None))
    }

    fn read_log_read_bytes_per_event(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.log_read_bytes_per_event.clone().unwrap_or(Ok(None))
    }

    fn read_pre_eventd_buffer_bytes(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.pre_eventd_buffer_bytes.clone().unwrap_or(Ok(None))
    }

    fn read_max_log_line_length(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.max_log_line_length.clone().unwrap_or(Ok(None))
    }

    fn read_max_log_buffer_per_service(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.max_log_buffer_per_service.clone().unwrap_or(Ok(None))
    }

    fn read_eventd_log_datagram_bytes(&mut self) -> Result<Option<u32>, BoundaryError> {
        self.eventd_log_datagram_bytes.clone().unwrap_or(Ok(None))
    }
}

#[derive(Debug, Clone)]
struct FixedClock {
    result: Result<u64, BoundaryError>,
    reads: usize,
}

impl FixedClock {
    fn at(observed_at_ns: u64) -> Self {
        Self {
            result: Ok(observed_at_ns),
            reads: 0,
        }
    }

    fn error(error: BoundaryError) -> Self {
        Self {
            result: Err(error),
            reads: 0,
        }
    }
}

impl Clock for FixedClock {
    fn monotonic_ns(&mut self) -> Result<u64, BoundaryError> {
        self.reads += 1;
        self.result.clone()
    }
}

fn settings() -> Phase2BootSettings {
    Phase2BootSettings {
        mode: BootMode::Full,
        max_parallel_starts: DEFAULT_MAX_PARALLEL_STARTS,
        ..Phase2BootSettings::default()
    }
}

fn service(name: &str, image_path: &str) -> ServiceDefinition {
    ServiceDefinition::simple_system_boot(name, image_path)
}

fn allocated_operation_id(sequence: usize) -> OperationId {
    OperationIdAllocator::new()
        .allocate_batch(sequence + 1, OBSERVED_AT_NS)
        .expect("operation ids")[sequence]
}
