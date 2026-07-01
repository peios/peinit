use std::collections::VecDeque;

use crate::boundary::{LinuxPowerButtonRead, LinuxPowerButtonReadError};
use crate::runtime::RuntimePowerButtonSource;

#[derive(Debug)]
pub(crate) struct FakePowerButtonSource {
    reads: VecDeque<Result<LinuxPowerButtonRead, LinuxPowerButtonReadError>>,
}

impl FakePowerButtonSource {
    pub(crate) fn new(
        reads: impl IntoIterator<Item = Result<LinuxPowerButtonRead, LinuxPowerButtonReadError>>,
    ) -> Self {
        Self {
            reads: reads.into_iter().collect(),
        }
    }

    pub(crate) fn would_block() -> Self {
        Self::new([])
    }
}

impl RuntimePowerButtonSource for FakePowerButtonSource {
    fn read_power_button(
        &mut self,
        fd: i32,
    ) -> Result<LinuxPowerButtonRead, LinuxPowerButtonReadError> {
        self.reads
            .pop_front()
            .unwrap_or(Ok(LinuxPowerButtonRead::WouldBlock))
            .map_err(|error| match error {
                LinuxPowerButtonReadError::StaleFd { .. } => {
                    LinuxPowerButtonReadError::StaleFd { fd }
                }
                LinuxPowerButtonReadError::Read { message, .. } => {
                    LinuxPowerButtonReadError::Read { fd, message }
                }
                LinuxPowerButtonReadError::ShortRead {
                    bytes, expected, ..
                } => LinuxPowerButtonReadError::ShortRead {
                    fd,
                    bytes,
                    expected,
                },
            })
    }
}
