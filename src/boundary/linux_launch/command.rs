use std::ffi::CString;

use crate::boundary::{BoundaryError, EnvironmentVariable};
use crate::job::JobRecord;

/// `LISTEN_PID=` plus room for a 32-bit PID and its NUL.
const LISTEN_PID_PREFIX: &[u8] = b"LISTEN_PID=";
const LISTEN_PID_BUFFER: usize = LISTEN_PID_PREFIX.len() + 11;

#[derive(Debug)]
pub(super) struct LaunchCommand {
    argv_strings: Vec<CString>,
    argv: Vec<*const libc::c_char>,
    working_directory: CString,
    env_strings: Vec<CString>,
    env: Vec<*const libc::c_char>,
    has_notify_socket: bool,
    /// The `LISTEN_PID` entry, filled in by the child after the clone.
    ///
    /// A conforming `sd_listen_fds(3)` checks `LISTEN_PID == getpid()` before
    /// trusting `LISTEN_FDS`, and treats a mismatch — including the variable
    /// being absent — as meaning no descriptors were passed. Without it the
    /// whole fd store was unreachable from any client using the convention it
    /// was designed around.
    ///
    /// The value has to be the *child's* PID, so it cannot be part of the
    /// environment the parent builds. The buffer is allocated here and the
    /// digits are written into it after the clone, where allocating would not
    /// be safe.
    listen_pid: Option<Box<[u8; LISTEN_PID_BUFFER]>>,
}

impl LaunchCommand {
    pub(super) fn new(
        job: &JobRecord,
        environment: &[EnvironmentVariable],
    ) -> Result<Self, BoundaryError> {
        let mut argv_strings = Vec::with_capacity(1 + job.arguments.len());
        argv_strings.push(c_string("image path", &job.image_path)?);
        if job.image_path.is_empty() {
            return Err(BoundaryError::Process("empty image path".to_string()));
        }
        for argument in &job.arguments {
            argv_strings.push(c_string("argument", argument)?);
        }
        let argv = pointer_array(&argv_strings);
        let working_directory = c_string("working directory", &job.working_directory)?;

        let mut env_strings = Vec::with_capacity(environment.len());
        let mut has_notify_socket = false;
        let mut injects_fds = false;
        for variable in environment {
            if variable.name == "NOTIFY_SOCKET" {
                has_notify_socket = true;
            }
            if variable.name == "LISTEN_FDS" {
                injects_fds = true;
            }
            env_strings.push(environment_c_string(variable)?);
        }

        // Only where descriptors are actually being injected. LISTEN_PID
        // without LISTEN_FDS says nothing, and PEI-334 is about the reverse
        // arriving from a configurable environment layer.
        let mut listen_pid = None;
        let mut env = pointer_array(&env_strings);
        if injects_fds {
            let mut buffer = Box::new([0u8; LISTEN_PID_BUFFER]);
            buffer[..LISTEN_PID_PREFIX.len()].copy_from_slice(LISTEN_PID_PREFIX);
            // The value is written in the child; until then the entry reads as
            // `LISTEN_PID=`, which no conforming client will match against its
            // own PID.
            let pointer = buffer.as_ptr() as *const libc::c_char;
            env.pop(); // the NULL terminator, restored below
            env.push(pointer);
            env.push(std::ptr::null());
            listen_pid = Some(buffer);
        }

        Ok(Self {
            argv_strings,
            argv,
            working_directory,
            env_strings,
            env,
            has_notify_socket,
            listen_pid,
        })
    }

    /// Write the child's own PID into the reserved `LISTEN_PID` entry.
    ///
    /// Called after the clone and before `execve`, where no allocation is safe.
    /// Writing decimal digits into a buffer this struct already owns is.
    pub(super) fn set_listen_pid(&mut self, pid: libc::pid_t) {
        let Some(buffer) = self.listen_pid.as_mut() else {
            return;
        };
        let mut digits = [0u8; 10];
        let mut value = pid.max(0) as u32;
        let mut count = 0;
        loop {
            digits[count] = b'0' + (value % 10) as u8;
            value /= 10;
            count += 1;
            if value == 0 {
                break;
            }
        }
        let start = LISTEN_PID_PREFIX.len();
        for i in 0..count {
            buffer[start + i] = digits[count - 1 - i];
        }
        buffer[start + count] = 0;
    }

    pub(super) fn program_ptr(&self) -> *const libc::c_char {
        self.argv_strings[0].as_ptr()
    }

    pub(super) fn argv_ptrs(&self) -> *const *const libc::c_char {
        debug_assert_eq!(self.argv.len(), self.argv_strings.len() + 1);
        self.argv.as_ptr()
    }

    pub(super) fn env_ptrs(&self) -> *const *const libc::c_char {
        let entries = self.env_strings.len() + usize::from(self.listen_pid.is_some());
        debug_assert_eq!(self.env.len(), entries + 1);
        self.env.as_ptr()
    }

    pub(super) fn working_directory_ptr(&self) -> *const libc::c_char {
        self.working_directory.as_ptr()
    }

    pub(super) fn environment_contains_notify_socket(&self) -> bool {
        self.has_notify_socket
    }
}

fn pointer_array(strings: &[CString]) -> Vec<*const libc::c_char> {
    let mut pointers = strings
        .iter()
        .map(|string| string.as_ptr())
        .collect::<Vec<_>>();
    pointers.push(std::ptr::null());
    pointers
}

fn c_string(label: &str, value: &str) -> Result<CString, BoundaryError> {
    CString::new(value).map_err(|_| {
        BoundaryError::Process(format!("{label} contains an interior NUL byte: {value:?}"))
    })
}

fn environment_c_string(variable: &EnvironmentVariable) -> Result<CString, BoundaryError> {
    if variable.name.is_empty() || variable.name.contains('=') {
        return Err(BoundaryError::Process(format!(
            "invalid environment variable name: {:?}",
            variable.name,
        )));
    }
    c_string(
        "environment variable",
        &format!("{}={}", variable.name, variable.value),
    )
}

#[cfg(test)]
mod tests {
    use crate::boundary::EnvironmentVariable;

    use super::LaunchCommand;

    #[test]
    fn launch_command_builds_argv_and_environment() {
        let mut job = crate::boundary::linux_launch::tests::test_job();
        job.image_path = "/sbin/app".to_string();
        job.arguments = vec!["--foreground".to_string(), "name=value".to_string()];
        job.working_directory = "/srv/app".to_string();
        let environment = vec![
            EnvironmentVariable {
                name: "NOTIFY_SOCKET".to_string(),
                value: "/run/notify.sock".to_string(),
            },
            EnvironmentVariable {
                name: "PATH".to_string(),
                value: "/sbin:/bin".to_string(),
            },
        ];

        let command = LaunchCommand::new(&job, &environment).expect("launch command");

        let argv = command
            .argv_strings
            .iter()
            .map(|value| value.to_str().expect("argv utf8"))
            .collect::<Vec<_>>();
        let env = command
            .env_strings
            .iter()
            .map(|value| value.to_str().expect("env utf8"))
            .collect::<Vec<_>>();
        assert_eq!(argv, vec!["/sbin/app", "--foreground", "name=value"]);
        assert_eq!(
            command.working_directory.to_str().expect("cwd utf8"),
            "/srv/app",
        );
        assert_eq!(
            env,
            vec!["NOTIFY_SOCKET=/run/notify.sock", "PATH=/sbin:/bin"]
        );
        assert!(command.argv.last().expect("argv null").is_null());
        assert!(command.env.last().expect("env null").is_null());
        assert!(command.environment_contains_notify_socket());
    }

    #[test]
    fn launch_command_rejects_invalid_environment_names() {
        let job = crate::boundary::linux_launch::tests::test_job();
        let environment = vec![EnvironmentVariable {
            name: "BAD=NAME".to_string(),
            value: "value".to_string(),
        }];

        assert!(LaunchCommand::new(&job, &environment).is_err());
    }

    /// TRM §5.4 — the child's step 9 is a confirmation, not a set: it checks
    /// that `NOTIFY_SOCKET` is present in the prebuilt environment and fails
    /// with a synthetic `EINVAL` when it is not. This is the predicate that
    /// confirmation reads. A guest cannot reach the failing branch — peinit
    /// always inserts `NOTIFY_SOCKET` in environment layer 4 and filters any
    /// configured value out of the layers below — so the discriminator is
    /// asserted here rather than from a VM.
    #[test]
    fn notify_socket_confirmation_discriminates_present_from_absent() {
        let job = crate::boundary::linux_launch::tests::test_job();

        let with = LaunchCommand::new(
            &job,
            &[EnvironmentVariable {
                name: "NOTIFY_SOCKET".to_string(),
                value: "/run/notify.sock".to_string(),
            }],
        )
        .expect("launch command");
        assert!(
            with.environment_contains_notify_socket(),
            "the confirmation sees NOTIFY_SOCKET when it is present",
        );

        let without = LaunchCommand::new(
            &job,
            &[EnvironmentVariable {
                name: "PATH".to_string(),
                value: "/sbin:/bin".to_string(),
            }],
        )
        .expect("launch command");
        assert!(
            !without.environment_contains_notify_socket(),
            "and reports its absence, which is what the child turns into EINVAL",
        );
    }
}

#[cfg(test)]
mod listen_pid_tests {
    use crate::boundary::EnvironmentVariable;

    use super::{LISTEN_PID_PREFIX, LaunchCommand};

    fn command_with(environment: Vec<EnvironmentVariable>) -> LaunchCommand {
        let mut job = crate::boundary::linux_launch::tests::test_job();
        job.image_path = "/sbin/app".to_string();
        LaunchCommand::new(&job, &environment).expect("launch command")
    }

    fn notify() -> EnvironmentVariable {
        EnvironmentVariable {
            name: "NOTIFY_SOCKET".to_string(),
            value: "/run/notify.sock".to_string(),
        }
    }

    fn listen_fds(count: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: "LISTEN_FDS".to_string(),
            value: count.to_string(),
        }
    }

    /// A conforming `sd_listen_fds(3)` checks `LISTEN_PID == getpid()` before
    /// trusting `LISTEN_FDS`, and treats a mismatch — the variable being absent
    /// included — as meaning no descriptors were passed. peinit never set it,
    /// so the whole fd store was unreachable from the software the convention
    /// exists for.
    #[test]
    fn a_launch_injecting_descriptors_reserves_listen_pid() {
        let mut command = command_with(vec![notify(), listen_fds("2")]);
        assert!(
            command.listen_pid.is_some(),
            "a launch with LISTEN_FDS must carry a LISTEN_PID slot"
        );

        command.set_listen_pid(4242);
        let buffer = command.listen_pid.as_ref().expect("slot");
        let text = std::ffi::CStr::from_bytes_until_nul(buffer.as_slice())
            .expect("NUL terminated")
            .to_str()
            .expect("utf8");
        assert_eq!(text, "LISTEN_PID=4242");
    }

    /// The entry has to be in the pointer array the child execs with, not only
    /// in the struct — and the array has to stay NULL-terminated.
    #[test]
    fn the_listen_pid_entry_is_in_the_environment_the_child_execs_with() {
        let mut command = command_with(vec![notify(), listen_fds("1")]);
        command.set_listen_pid(7);

        let mut seen = Vec::new();
        let mut cursor = command.env_ptrs();
        // SAFETY: the array is NULL-terminated by construction.
        unsafe {
            while !(*cursor).is_null() {
                seen.push(
                    std::ffi::CStr::from_ptr(*cursor)
                        .to_str()
                        .expect("utf8")
                        .to_string(),
                );
                cursor = cursor.add(1);
            }
        }
        assert!(
            seen.contains(&"LISTEN_PID=7".to_string()),
            "the child's environment is {seen:?}"
        );
        assert!(seen.contains(&"LISTEN_FDS=1".to_string()));
    }

    /// No descriptors, no LISTEN_PID. Setting it alone would say nothing, and
    /// the environment must not grow an entry for a launch that injects
    /// nothing.
    #[test]
    fn a_launch_injecting_nothing_carries_no_listen_pid() {
        let mut command = command_with(vec![notify()]);
        assert!(command.listen_pid.is_none());
        command.set_listen_pid(4242); // a no-op rather than a panic

        let mut cursor = command.env_ptrs();
        unsafe {
            while !(*cursor).is_null() {
                let text = std::ffi::CStr::from_ptr(*cursor).to_str().expect("utf8");
                assert!(!text.starts_with("LISTEN_PID"), "unexpected {text}");
                cursor = cursor.add(1);
            }
        }
    }

    /// A single digit and the widest PID both have to fit and terminate.
    #[test]
    fn listen_pid_renders_the_whole_range() {
        for pid in [1i32, 9, 10, 99999, i32::MAX] {
            let mut command = command_with(vec![notify(), listen_fds("1")]);
            command.set_listen_pid(pid);
            let buffer = command.listen_pid.as_ref().expect("slot");
            let text = std::ffi::CStr::from_bytes_until_nul(buffer.as_slice())
                .expect("NUL terminated")
                .to_str()
                .expect("utf8");
            assert_eq!(text, format!("LISTEN_PID={pid}"));
            assert!(text.as_bytes().starts_with(LISTEN_PID_PREFIX));
        }
    }
}
