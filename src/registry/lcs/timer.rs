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

pub(super) fn queue_timer_last_run_write(
    service: String,
    schedule: String,
    storage: TimerLastRunStorage,
    timestamp_realtime_ns: u64,
) -> Result<(), LcsTimerLastRunError> {
    match unsafe { libc::fork() } {
        -1 => Err(LcsTimerLastRunError::Queue {
            source: io::Error::last_os_error(),
        }),
        0 => {
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
        _ => Ok(()),
    }
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
