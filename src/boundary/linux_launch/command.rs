use std::ffi::CString;

use crate::boundary::{BoundaryError, EnvironmentVariable};
use crate::job::JobRecord;

#[derive(Debug)]
pub(super) struct LaunchCommand {
    argv_strings: Vec<CString>,
    argv: Vec<*const libc::c_char>,
    working_directory: CString,
    env_strings: Vec<CString>,
    env: Vec<*const libc::c_char>,
    has_notify_socket: bool,
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
        for variable in environment {
            if variable.name == "NOTIFY_SOCKET" {
                has_notify_socket = true;
            }
            env_strings.push(environment_c_string(variable)?);
        }
        let env = pointer_array(&env_strings);

        Ok(Self {
            argv_strings,
            argv,
            working_directory,
            env_strings,
            env,
            has_notify_socket,
        })
    }

    pub(super) fn program_ptr(&self) -> *const libc::c_char {
        self.argv_strings[0].as_ptr()
    }

    pub(super) fn argv_ptrs(&self) -> *const *const libc::c_char {
        debug_assert_eq!(self.argv.len(), self.argv_strings.len() + 1);
        self.argv.as_ptr()
    }

    pub(super) fn env_ptrs(&self) -> *const *const libc::c_char {
        debug_assert_eq!(self.env.len(), self.env_strings.len() + 1);
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
}
