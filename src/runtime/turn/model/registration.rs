use crate::boundary::LinuxEpoll;
use crate::runtime::RuntimeEventSource;

pub trait RuntimeEventRegistrar {
    fn register_source(
        &mut self,
        fd: i32,
        source: RuntimeEventSource,
    ) -> Result<(), RuntimeEventRegistrationError>;

    fn unregister_source(&mut self, fd: i32) -> Result<(), RuntimeEventRegistrationError>;
}

impl RuntimeEventRegistrar for LinuxEpoll {
    fn register_source(
        &mut self,
        fd: i32,
        source: RuntimeEventSource,
    ) -> Result<(), RuntimeEventRegistrationError> {
        self.register_read(fd, source.token()).map_err(|error| {
            RuntimeEventRegistrationError::Register {
                fd,
                source,
                message: format!("{error:?}"),
            }
        })
    }

    fn unregister_source(&mut self, fd: i32) -> Result<(), RuntimeEventRegistrationError> {
        self.unregister(fd)
            .map_err(|error| RuntimeEventRegistrationError::Unregister {
                fd,
                message: format!("{error:?}"),
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEventRegistrationError {
    Register {
        fd: i32,
        source: RuntimeEventSource,
        message: String,
    },
    Unregister {
        fd: i32,
        message: String,
    },
}
