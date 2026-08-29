const KIND_PID1_SIGNAL: u32 = 1;
const KIND_CONTROL_LISTENER: u32 = 2;
const KIND_CONTROL_CONNECTION: u32 = 3;
const KIND_SHUTDOWN_DEADLINE_TIMER: u32 = 4;
const KIND_NOTIFY_SOCKET: u32 = 5;
const KIND_LIFECYCLE_DEADLINE_TIMER: u32 = 6;
const KIND_SERVICE_LOG_PIPE: u32 = 7;
const KIND_CALENDAR_TIMER: u32 = 9;
const KIND_FILESYSTEM_CHECK_HELPER: u32 = 10;
const KIND_FILESYSTEM_CHECK_HELPER_EXIT: u32 = 11;
const KIND_REGISTRY_WATCH: u32 = 12;
const KIND_PROCESS_SETUP: u32 = 13;
const KIND_POWER_BUTTON: u32 = 14;
const KIND_JOBS_LISTENER: u32 = 8;
const KIND_JOBS_CONNECTION: u32 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEventSource {
    Pid1Signal,
    ControlListener,
    ControlConnection { fd: i32 },
    ShutdownDeadlineTimer,
    NotifySocket,
    LifecycleDeadlineTimer,
    ServiceLogPipe { fd: i32 },
    CalendarTimer { fd: i32 },
    FilesystemCheckHelper { result_fd: i32 },
    FilesystemCheckHelperExit { pidfd: i32 },
    RegistryWatch { fd: i32 },
    ProcessSetup { fd: i32 },
    PowerButton { fd: i32 },
    JobsListener,
    JobsConnection { fd: i32 },
}

impl RuntimeEventSource {
    pub const fn token(self) -> u64 {
        match self {
            Self::Pid1Signal => encode_token(KIND_PID1_SIGNAL, 0),
            Self::ControlListener => encode_token(KIND_CONTROL_LISTENER, 0),
            Self::ControlConnection { fd } => encode_token(KIND_CONTROL_CONNECTION, fd as u32),
            Self::ShutdownDeadlineTimer => encode_token(KIND_SHUTDOWN_DEADLINE_TIMER, 0),
            Self::NotifySocket => encode_token(KIND_NOTIFY_SOCKET, 0),
            Self::LifecycleDeadlineTimer => encode_token(KIND_LIFECYCLE_DEADLINE_TIMER, 0),
            Self::ServiceLogPipe { fd } => encode_token(KIND_SERVICE_LOG_PIPE, fd as u32),
            Self::CalendarTimer { fd } => encode_token(KIND_CALENDAR_TIMER, fd as u32),
            Self::FilesystemCheckHelper { result_fd } => {
                encode_token(KIND_FILESYSTEM_CHECK_HELPER, result_fd as u32)
            }
            Self::FilesystemCheckHelperExit { pidfd } => {
                encode_token(KIND_FILESYSTEM_CHECK_HELPER_EXIT, pidfd as u32)
            }
            Self::RegistryWatch { fd } => encode_token(KIND_REGISTRY_WATCH, fd as u32),
            Self::ProcessSetup { fd } => encode_token(KIND_PROCESS_SETUP, fd as u32),
            Self::PowerButton { fd } => encode_token(KIND_POWER_BUTTON, fd as u32),
            Self::JobsListener => encode_token(KIND_JOBS_LISTENER, 0),
            Self::JobsConnection { fd } => encode_token(KIND_JOBS_CONNECTION, fd as u32),
        }
    }

    pub fn control_connection(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::ControlConnection { fd })
    }

    pub fn service_log_pipe(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::ServiceLogPipe { fd })
    }

    pub fn calendar_timer(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::CalendarTimer { fd })
    }

    pub fn filesystem_check_helper(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::FilesystemCheckHelper { result_fd: fd })
    }

    pub fn filesystem_check_helper_exit(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::FilesystemCheckHelperExit { pidfd: fd })
    }

    pub fn registry_watch(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::RegistryWatch { fd })
    }

    pub fn process_setup(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::ProcessSetup { fd })
    }

    pub fn power_button(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::PowerButton { fd })
    }

    pub fn jobs_connection(fd: i32) -> Result<Self, RuntimeEventSourceDecodeError> {
        source_from_fd(fd, |fd| Self::JobsConnection { fd })
    }

    pub fn from_token(token: u64) -> Result<Self, RuntimeEventSourceDecodeError> {
        let kind = (token >> 32) as u32;
        let value = (token & u64::from(u32::MAX)) as u32;
        match kind {
            KIND_PID1_SIGNAL if value == 0 => Ok(Self::Pid1Signal),
            KIND_CONTROL_LISTENER if value == 0 => Ok(Self::ControlListener),
            KIND_CONTROL_CONNECTION => {
                source_from_token_fd(value, |fd| Self::ControlConnection { fd })
            }
            KIND_SHUTDOWN_DEADLINE_TIMER if value == 0 => Ok(Self::ShutdownDeadlineTimer),
            KIND_NOTIFY_SOCKET if value == 0 => Ok(Self::NotifySocket),
            KIND_LIFECYCLE_DEADLINE_TIMER if value == 0 => Ok(Self::LifecycleDeadlineTimer),
            KIND_SERVICE_LOG_PIPE => source_from_token_fd(value, |fd| Self::ServiceLogPipe { fd }),
            KIND_CALENDAR_TIMER => source_from_token_fd(value, |fd| Self::CalendarTimer { fd }),
            KIND_FILESYSTEM_CHECK_HELPER => {
                source_from_token_fd(value, |fd| Self::FilesystemCheckHelper { result_fd: fd })
            }
            KIND_FILESYSTEM_CHECK_HELPER_EXIT => {
                source_from_token_fd(value, |fd| Self::FilesystemCheckHelperExit { pidfd: fd })
            }
            KIND_REGISTRY_WATCH => source_from_token_fd(value, |fd| Self::RegistryWatch { fd }),
            KIND_PROCESS_SETUP => source_from_token_fd(value, |fd| Self::ProcessSetup { fd }),
            KIND_POWER_BUTTON => source_from_token_fd(value, |fd| Self::PowerButton { fd }),
            KIND_JOBS_LISTENER if value == 0 => Ok(Self::JobsListener),
            KIND_JOBS_CONNECTION => source_from_token_fd(value, |fd| Self::JobsConnection { fd }),
            _ => Err(RuntimeEventSourceDecodeError::UnknownToken { token }),
        }
    }
}

fn source_from_fd(
    fd: i32,
    source: impl FnOnce(i32) -> RuntimeEventSource,
) -> Result<RuntimeEventSource, RuntimeEventSourceDecodeError> {
    if fd < 0 {
        Err(RuntimeEventSourceDecodeError::NegativeFd { fd })
    } else {
        Ok(source(fd))
    }
}

fn source_from_token_fd(
    value: u32,
    source: impl FnOnce(i32) -> RuntimeEventSource,
) -> Result<RuntimeEventSource, RuntimeEventSourceDecodeError> {
    let fd =
        i32::try_from(value).map_err(|_| RuntimeEventSourceDecodeError::FdOutOfRange { value })?;
    Ok(source(fd))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEventSourceDecodeError {
    UnknownToken { token: u64 },
    NegativeFd { fd: i32 },
    FdOutOfRange { value: u32 },
}

const fn encode_token(kind: u32, value: u32) -> u64 {
    ((kind as u64) << 32) | value as u64
}

#[cfg(test)]
mod tests {
    use super::{RuntimeEventSource, RuntimeEventSourceDecodeError};

    #[test]
    fn event_source_tokens_round_trip() {
        let sources = [
            RuntimeEventSource::Pid1Signal,
            RuntimeEventSource::ControlListener,
            RuntimeEventSource::ControlConnection { fd: 42 },
            RuntimeEventSource::ShutdownDeadlineTimer,
            RuntimeEventSource::NotifySocket,
            RuntimeEventSource::LifecycleDeadlineTimer,
            RuntimeEventSource::ServiceLogPipe { fd: 43 },
            RuntimeEventSource::CalendarTimer { fd: 45 },
            RuntimeEventSource::FilesystemCheckHelper { result_fd: 46 },
            RuntimeEventSource::FilesystemCheckHelperExit { pidfd: 47 },
            RuntimeEventSource::RegistryWatch { fd: 48 },
            RuntimeEventSource::ProcessSetup { fd: 49 },
            RuntimeEventSource::PowerButton { fd: 50 },
            RuntimeEventSource::JobsListener,
            RuntimeEventSource::JobsConnection { fd: 51 },
        ];

        for source in sources {
            assert_eq!(
                RuntimeEventSource::from_token(source.token()).expect("decode"),
                source,
            );
        }
    }

    #[test]
    fn control_connection_source_rejects_negative_fd() {
        assert_eq!(
            RuntimeEventSource::control_connection(-1),
            Err(RuntimeEventSourceDecodeError::NegativeFd { fd: -1 }),
        );
    }

    #[test]
    fn calendar_timer_source_rejects_negative_fd() {
        assert_eq!(
            RuntimeEventSource::calendar_timer(-1),
            Err(RuntimeEventSourceDecodeError::NegativeFd { fd: -1 }),
        );
    }

    #[test]
    fn filesystem_check_helper_source_rejects_negative_fd() {
        assert_eq!(
            RuntimeEventSource::filesystem_check_helper(-1),
            Err(RuntimeEventSourceDecodeError::NegativeFd { fd: -1 }),
        );
    }

    #[test]
    fn filesystem_check_helper_exit_source_rejects_negative_fd() {
        assert_eq!(
            RuntimeEventSource::filesystem_check_helper_exit(-1),
            Err(RuntimeEventSourceDecodeError::NegativeFd { fd: -1 }),
        );
    }

    #[test]
    fn registry_watch_source_rejects_negative_fd() {
        assert_eq!(
            RuntimeEventSource::registry_watch(-1),
            Err(RuntimeEventSourceDecodeError::NegativeFd { fd: -1 }),
        );
    }

    #[test]
    fn process_setup_source_rejects_negative_fd() {
        assert_eq!(
            RuntimeEventSource::process_setup(-1),
            Err(RuntimeEventSourceDecodeError::NegativeFd { fd: -1 }),
        );
    }
}
