use super::DEFAULT_PRE_EVENTD_BUFFER_BYTES;

pub const DEFAULT_MAX_LOG_LINE_BYTES: usize = 8192;
pub const DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES: usize = 65_536;
pub const DEFAULT_LOG_READ_BYTES_PER_EVENT: usize = 16 * 1024;
/// PSPU's portable producer ceiling. Production configuration never overrides it.
pub const DEFAULT_EVENTD_LOG_DATAGRAM_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLogConfig {
    pub max_line_bytes: usize,
    pub read_bytes_per_event: usize,
    pub pre_eventd_buffer_bytes: usize,
    pub max_buffer_per_service_bytes: usize,
    /// Fixed to the PSPU portable ceiling outside tests.
    pub(crate) eventd_log_datagram_bytes: usize,
}

impl Default for RuntimeLogConfig {
    fn default() -> Self {
        Self {
            max_line_bytes: DEFAULT_MAX_LOG_LINE_BYTES,
            read_bytes_per_event: DEFAULT_LOG_READ_BYTES_PER_EVENT,
            pre_eventd_buffer_bytes: DEFAULT_PRE_EVENTD_BUFFER_BYTES,
            max_buffer_per_service_bytes: DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
            eventd_log_datagram_bytes: DEFAULT_EVENTD_LOG_DATAGRAM_BYTES,
        }
    }
}
