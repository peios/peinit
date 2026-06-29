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

    pub fn update_config(&mut self, config: RuntimeLogConfig) {
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
