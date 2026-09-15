use crate::logging::ServiceLogRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeLogPipeTurn {
    Read {
        fd: i32,
        records: Vec<ServiceLogRecord>,
        closed: bool,
        would_block: bool,
        buffered_records: usize,
        /// A submitted job's output sink dropped its first line on this
        /// read; reported once per job (PSPU §7.9).
        output_dropped: Option<crate::ids::JobId>,
    },
    Stale {
        fd: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEventdLogFlush {
    pub eventd_active: bool,
    pub socket_path_configured: bool,
    pub attempted_records: usize,
    pub sent_records: usize,
    pub buffered_records: usize,
    /// Records given up on because they cannot fit one eventd datagram --
    /// oversized for the socket, or a single record over the ceiling. Not
    /// buffered and not retried: retrying reproduces the same refusal, and
    /// doing so every turn stopped log delivery for the rest of the boot
    /// (PEI-807). The runtime says so on the console once per flush.
    pub discarded_records: usize,
    pub error: Option<String>,
}

impl RuntimeEventdLogFlush {
    pub fn not_configured(buffered_records: usize) -> Self {
        Self::unavailable(false, false, buffered_records)
    }

    pub fn unavailable(
        eventd_active: bool,
        socket_path_configured: bool,
        buffered_records: usize,
    ) -> Self {
        Self {
            eventd_active,
            socket_path_configured,
            attempted_records: 0,
            sent_records: 0,
            buffered_records,
            discarded_records: 0,
            error: None,
        }
    }
}

impl Default for RuntimeEventdLogFlush {
    fn default() -> Self {
        Self::not_configured(0)
    }
}
