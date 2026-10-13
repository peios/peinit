use std::{fmt, io};

use crate::timer::state::{
    LAST_TIMER_RUN_VALUE_NAME, TIMER_STATE_SUBKEY_NAME, TimerLastRunStorage,
    encode_timer_schedule_value_name,
};

use super::SERVICES_ROOT_KEY;

#[derive(Debug)]
pub(super) enum LcsTimerLastRunError {
    Open {
        key_path: String,
        source: peios::Error,
    },
    Create {
        key_path: String,
        source: peios::Error,
    },
    Read {
        key_path: String,
        value_name: String,
        source: peios::Error,
    },
    Write {
        key_path: String,
        value_name: String,
        source: peios::Error,
    },
    Queue {
        source: io::Error,
    },
    InvalidType {
        key_path: String,
        value_name: String,
        actual: u32,
    },
    InvalidLength {
        key_path: String,
        value_name: String,
        actual_len: usize,
    },
}

impl fmt::Display for LcsTimerLastRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { key_path, source } => {
                write!(f, "open timer state key {key_path:?}: {source:?}")
            }
            Self::Create { key_path, source } => {
                write!(f, "create timer state key {key_path:?}: {source:?}")
            }
            Self::Read {
                key_path,
                value_name,
                source,
            } => write!(
                f,
                "read timer state value {value_name:?} under {key_path:?}: {source:?}",
            ),
            Self::Write {
                key_path,
                value_name,
                source,
            } => write!(
                f,
                "write timer state value {value_name:?} under {key_path:?}: {source:?}",
            ),
            Self::Queue { source } => write!(f, "queue timer state write helper: {source}"),
            Self::InvalidType {
                key_path,
                value_name,
                actual,
            } => write!(
                f,
                "timer state value {value_name:?} under {key_path:?} has type {actual}, expected REG_QWORD",
            ),
            Self::InvalidLength {
                key_path,
                value_name,
                actual_len,
            } => write!(
                f,
                "timer state value {value_name:?} under {key_path:?} has {actual_len} bytes, expected 8",
            ),
        }
    }
}

impl std::error::Error for LcsTimerLastRunError {}

pub(super) fn read_timer_last_run(
    service: &str,
    schedule: &str,
    storage: TimerLastRunStorage,
) -> Result<Option<u64>, LcsTimerLastRunError> {
    use peios::registry::{Key, KeyAccess, OpenFlags, ValueType};

    let (key_path, value_name) = timer_last_run_location(service, schedule, storage);
    let key = match Key::open(
        None,
        &key_path,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    ) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(None),
        Err(source) => return Err(LcsTimerLastRunError::Open { key_path, source }),
    };
    let value = match key.query_value(&value_name, None) {
        Ok(value) => value,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(None),
        Err(source) => {
            return Err(LcsTimerLastRunError::Read {
                key_path,
                value_name: decode_timer_value_name(value_name),
                source,
            });
        }
    };
    if value.ty != ValueType::QWORD {
        return Err(LcsTimerLastRunError::InvalidType {
            key_path,
            value_name: decode_timer_value_name(value_name),
            actual: value.ty.0,
        });
    }
    let bytes: [u8; 8] =
        value
            .data
            .as_slice()
            .try_into()
            .map_err(|_| LcsTimerLastRunError::InvalidLength {
                key_path: key_path.clone(),
                value_name: decode_timer_value_name(value_name.clone()),
                actual_len: value.data.len(),
            })?;
    Ok(Some(u64::from_le_bytes(bytes)))
}

pub(super) fn write_timer_last_run(
    service: &str,
    schedule: &str,
    storage: TimerLastRunStorage,
    timestamp_realtime_ns: u64,
) -> Result<(), LcsTimerLastRunError> {
    use peios::registry::{CreateFlags, Key, KeyAccess, OpenFlags, ValueType};

    let (key_path, value_name) = timer_last_run_location(service, schedule, storage);
    let key = match storage {
        TimerLastRunStorage::SingleTimer => {
            Key::open(None, &key_path, KeyAccess::SET_VALUE, OpenFlags::default()).map_err(
                |source| LcsTimerLastRunError::Open {
                    key_path: key_path.clone(),
                    source,
                },
            )?
        }
        TimerLastRunStorage::PerTrigger => Key::create(
            None,
            &key_path,
            KeyAccess::SET_VALUE,
            CreateFlags::default(),
            None,
            None,
        )
        .map(|(key, _)| key)
        .map_err(|source| LcsTimerLastRunError::Create {
            key_path: key_path.clone(),
            source,
        })?,
    };
    let data = timestamp_realtime_ns.to_le_bytes();
    key.set_value(&value_name, ValueType::QWORD, &data)
        .call()
        .map_err(|source| LcsTimerLastRunError::Write {
            key_path,
            value_name: decode_timer_value_name(value_name),
            source,
        })
}

/// Perform the last-run write in a forked child, returning its pid.
///
/// §9.1 requires this to be asynchronous — peinit's event loop cannot wait on
/// registryd, which is a service it supervises — and a fork is how peinit gets
/// that today. The pid is returned rather than discarded so the caller can
/// match the child's exit status back to this write; before that, a failed
/// write existed only as an exit status the generic reaper threw away as an
/// untracked child (PEI-369).
pub(super) fn queue_timer_last_run_write(
    service: String,
    schedule: String,
    storage: TimerLastRunStorage,
    timestamp_realtime_ns: u64,
) -> Result<u32, LcsTimerLastRunError> {
    match unsafe { libc::fork() } {
        -1 => Err(LcsTimerLastRunError::Queue {
            source: io::Error::last_os_error(),
        }),
        0 => {
            if !close_timer_writer_inherited_fds() {
                // The parent reports this through the normal failed-write reap.
                unsafe { libc::_exit(1) }
            }
            let exit_code = if write_timer_last_run(
                &service,
                &schedule,
                storage,
                timestamp_realtime_ns,
            )
            .is_ok()
            {
                0
            } else {
                1
            };
            unsafe { libc::_exit(exit_code) }
        }
        pid => Ok(pid as u32),
    }
}

fn close_timer_writer_inherited_fds() -> bool {
    // PID 1's supervision handles are unused here. Both storage variants open
    // a fresh absolute registry key after this call; the SDK caches no client
    // descriptor. Keep standard streams and avoid allocation in the cleanup.
    // The caller must _exit, never drop inherited OwnedFd wrappers after reuse.
    unsafe { libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 0_u32) == 0 }
}

fn timer_last_run_location(
    service: &str,
    schedule: &str,
    storage: TimerLastRunStorage,
) -> (String, Vec<u8>) {
    match storage {
        TimerLastRunStorage::SingleTimer => (
            format!("{SERVICES_ROOT_KEY}\\{service}"),
            LAST_TIMER_RUN_VALUE_NAME.to_vec(),
        ),
        TimerLastRunStorage::PerTrigger => (
            format!("{SERVICES_ROOT_KEY}\\{service}\\{TIMER_STATE_SUBKEY_NAME}"),
            encode_timer_schedule_value_name(schedule).into_bytes(),
        ),
    }
}

fn decode_timer_value_name(value_name: Vec<u8>) -> String {
    String::from_utf8(value_name).unwrap_or_else(|error| {
        format!(
            "<invalid utf-8 timer value name: {} bytes>",
            error.into_bytes().len()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::state::TimerLastRunStorage;

    #[test]
    fn timer_writer_closes_inherited_fds_without_changing_parent_or_stdio() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        let source = std::fs::File::open("/dev/null").expect("sentinel source");
        let raw = unsafe { libc::fcntl(source.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 64) };
        assert!(raw >= 64, "allocate a high inherited descriptor");
        let sentinel = unsafe { OwnedFd::from_raw_fd(raw) };
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork the descriptor cleanup probe");
        if pid == 0 {
            // Only syscalls and stack values after fork: the test harness may
            // have other threads. Communicate through exit status, not a pipe
            // which is itself supposed to be closed by the cleanup.
            let before = unsafe {
                [
                    libc::fcntl(0, libc::F_GETFD),
                    libc::fcntl(1, libc::F_GETFD),
                    libc::fcntl(2, libc::F_GETFD),
                ]
            };
            let cleaned = close_timer_writer_inherited_fds();
            let closed = unsafe {
                libc::fcntl(raw, libc::F_GETFD) == -1 && *libc::__errno_location() == libc::EBADF
            };
            let unchanged =
                (0..3).all(|fd| unsafe { libc::fcntl(fd, libc::F_GETFD) == before[fd as usize] });
            unsafe { libc::_exit(if cleaned && closed && unchanged { 0 } else { 1 }) }
        }
        let mut status = 0;
        loop {
            let reaped = unsafe { libc::waitpid(pid, &mut status, 0) };
            if reaped == pid {
                break;
            }
            assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EINTR));
        }
        assert!(libc::WIFEXITED(status), "probe exited normally");
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "child descriptors were isolated"
        );
        assert!(unsafe { libc::fcntl(sentinel.as_raw_fd(), libc::F_GETFD) } >= 0);
    }

    /// §9.2: the write is performed in a forked child, so the event loop never
    /// waits on the registry.
    ///
    /// The proof is a real fork: the call returns the pid of a *distinct*
    /// process rather than doing the write inline and returning the caller's
    /// own pid. What that child then does — succeed or fail against the
    /// registry — is beside this point; it is killed and reaped here so the
    /// test leaves nothing behind, precisely because the parent did not wait
    /// for it. That is the whole claim: the parent returned a child's pid at
    /// once.
    #[test]
    fn the_last_run_write_is_performed_in_a_forked_child() {
        let parent = std::process::id();

        let pid = queue_timer_last_run_write(
            "pt-timer".to_string(),
            "daily UTC".to_string(),
            TimerLastRunStorage::SingleTimer,
            1_000_000,
        )
        .expect("the write is queued");

        assert!(pid > 1, "a real child pid was returned, not a sentinel");
        assert_ne!(
            pid, parent,
            "the write runs in a forked child, not inline in the caller's process",
        );

        // The child is the writer, not this process; kill and reap it so the
        // test leaves no process behind whatever the child was doing. SIGKILL
        // is uncatchable, so this terminates it regardless.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
            let mut status = 0;
            let reaped = libc::waitpid(pid as libc::pid_t, &mut status, 0);
            assert_eq!(reaped, pid as libc::pid_t, "the forked child is reaped");
        }
    }
}
