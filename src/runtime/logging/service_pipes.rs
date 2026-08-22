use std::collections::BTreeMap;

use crate::logging::{PreEventdLogBuffer, RuntimeLogConfig, ServiceLogRecord};

use super::pipe::ServiceLogPipe;

mod read;
mod registration;

#[derive(Debug)]
pub struct RuntimeServiceLogPipes {
    pub(in crate::runtime::logging) config: RuntimeLogConfig,
    pub(in crate::runtime::logging) pipes: BTreeMap<i32, ServiceLogPipe>,
    pub(in crate::runtime::logging) pre_eventd: PreEventdLogBuffer,
    pub(in crate::runtime::logging) eventd_socket_path: Option<String>,
}

impl RuntimeServiceLogPipes {
    pub fn new(config: RuntimeLogConfig) -> Self {
        Self {
            pre_eventd: PreEventdLogBuffer::new(config.pre_eventd_buffer_bytes),
            eventd_socket_path: None,
            config,
            pipes: BTreeMap::new(),
        }
    }

    pub fn buffered_records(&self) -> Vec<ServiceLogRecord> {
        self.pre_eventd.records()
    }

    pub fn active_pipe_count(&self) -> usize {
        self.pipes.len()
    }

    pub fn config(&self) -> &RuntimeLogConfig {
        &self.config
    }

    /// Adopt a reloaded log config, including the pre-eventd buffer capacity.
    ///
    /// Resizing here rather than only in `new` is what makes
    /// `Machine\System\Init\PreEventdBuffer` take effect at all: the Linux
    /// runtime builds its pipes with `Default` before Phase 2 has read the
    /// registry, and every turn then syncs the effective config through this
    /// method. Without the resize the buffer stayed at the compiled-in default
    /// for the life of the boot, however the key was set.
    pub fn update_config(&mut self, config: RuntimeLogConfig) {
        self.pre_eventd
            .set_capacity_bytes(config.pre_eventd_buffer_bytes);
        self.config = config;
    }

    pub fn eventd_forwarding_enabled(&self) -> bool {
        self.eventd_socket_path.is_some()
    }
}

impl Default for RuntimeServiceLogPipes {
    fn default() -> Self {
        Self::new(RuntimeLogConfig::default())
    }
}
