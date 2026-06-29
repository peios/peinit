use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChildExecReport {
    ExecSucceeded,
    Failed(ChildFailure),
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChildFailure {
    pub step: ChildFailureStep,
    pub errno: i32,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChildFailureStep {
    DupConsole = 1,
    Exec = 2,
}

impl ChildFailureStep {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::DupConsole => "dup-console",
            Self::Exec => "exec",
        }
    }
}

pub(super) fn read_child_exec_report(fd: i32) -> io::Result<ChildExecReport> {
    let mut failure = ChildFailure {
        step: ChildFailureStep::Exec,
        errno: 0,
    };
    let expected = std::mem::size_of::<ChildFailure>();
    let mut read_total = 0usize;
    while read_total < expected {
        let target = unsafe {
            (&mut failure as *mut ChildFailure)
                .cast::<u8>()
                .add(read_total)
        };
        let read = unsafe { libc::read(fd, target.cast(), expected - read_total) };
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if read == 0 {
            if read_total == 0 {
                return Ok(ChildExecReport::ExecSucceeded);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short child exec failure report",
            ));
        }
        read_total += read as usize;
    }
    Ok(ChildExecReport::Failed(failure))
}

pub(super) fn write_child_failure(fd: i32, step: ChildFailureStep, errno: i32) -> ! {
    let failure = ChildFailure { step, errno };
    let _ = unsafe {
        libc::write(
            fd,
            (&failure as *const ChildFailure).cast::<libc::c_void>(),
            std::mem::size_of::<ChildFailure>(),
        )
    };
    unsafe { libc::_exit(127) }
}
