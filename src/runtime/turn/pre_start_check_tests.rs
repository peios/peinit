use crate::boundary::LaunchedFilesystemCheckHelper;
use crate::ids::OperationIdAllocator;
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource, RuntimeWorkPumpTurn,
};
use crate::service::{ServiceCheck, ServiceCheckKind};
use crate::supervisor::SupervisorFilesystemCheckLaunchDispatch;

use super::pre_start_check::register_filesystem_check_helper_sources;

#[test]
fn helper_registration_rolls_back_result_fd_when_pidfd_registration_fails() {
    let mut registrar = FailOnFdRegistrar::new(80);
    let err = register_filesystem_check_helper_sources(
        &RuntimeWorkPumpTurn {
            filesystem_check_launches: vec![SupervisorFilesystemCheckLaunchDispatch {
                helper: launched_helper(),
            }],
            ..RuntimeWorkPumpTurn::default()
        },
        &mut registrar,
    )
    .expect_err("pidfd registration fails");

    assert!(matches!(
        err,
        RuntimeEventRegistrationError::Register { fd: 80, .. }
    ));
    assert_eq!(
        registrar.registrations,
        vec![
            (
                81,
                RuntimeEventSource::FilesystemCheckHelper { result_fd: 81 }
            ),
            (
                80,
                RuntimeEventSource::FilesystemCheckHelperExit { pidfd: 80 },
            ),
        ],
    );
    assert_eq!(registrar.unregistrations, vec![81]);
}

#[derive(Debug)]
struct FailOnFdRegistrar {
    fail_fd: i32,
    registrations: Vec<(i32, RuntimeEventSource)>,
    unregistrations: Vec<i32>,
}

impl FailOnFdRegistrar {
    fn new(fail_fd: i32) -> Self {
        Self {
            fail_fd,
            registrations: Vec::new(),
            unregistrations: Vec::new(),
        }
    }
}

impl RuntimeEventRegistrar for FailOnFdRegistrar {
    fn register_source(
        &mut self,
        fd: i32,
        source: RuntimeEventSource,
    ) -> Result<(), RuntimeEventRegistrationError> {
        self.registrations.push((fd, source));
        if fd == self.fail_fd {
            return Err(RuntimeEventRegistrationError::Register {
                fd,
                source,
                message: "register failed".to_string(),
            });
        }
        Ok(())
    }

    fn unregister_source(&mut self, fd: i32) -> Result<(), RuntimeEventRegistrationError> {
        self.unregistrations.push(fd);
        Ok(())
    }
}

fn launched_helper() -> LaunchedFilesystemCheckHelper {
    LaunchedFilesystemCheckHelper {
        service: "app".to_string(),
        operation_id: OperationIdAllocator::new()
            .allocate_batch(1, 1)
            .expect("operation id")[0],
        checks: vec![ServiceCheck {
            kind: ServiceCheckKind::Path,
            argument: "/etc/app.enabled".to_string(),
        }],
        pid: 4242,
        pidfd: 80,
        result_fd: 81,
        cgroup_id: "/sys/fs/cgroup/peinit/app/checks".to_string(),
    }
}
