use std::os::fd::FromRawFd;

use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::job::JobEvent;
use crate::logging::LogStream;
use crate::runtime::{
    RuntimeEventRegistrar, RuntimeEventRegistrationError, RuntimeEventSource, RuntimeWorkPumpTurn,
};

use super::RuntimeServiceLogPipes;
use crate::runtime::logging::origin::origin_for_job;
use crate::runtime::logging::pipe::ServiceLogPipe;

impl RuntimeServiceLogPipes {
    pub fn register_work_pump_turn<R>(
        &mut self,
        turn: &RuntimeWorkPumpTurn,
        registrar: &mut R,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventRegistrationError>
    where
        R: RuntimeEventRegistrar + ?Sized,
    {
        let mut registrations = Vec::new();
        for dispatch in &turn.start_hook_launches {
            self.register_launch(&dispatch.launch, registrar, &mut registrations)?;
        }
        for dispatch in &turn.post_hook_launches {
            self.register_launch(&dispatch.launch, registrar, &mut registrations)?;
        }
        for dispatch in &turn.control_launches {
            self.register_launch(&dispatch.launch, registrar, &mut registrations)?;
        }
        for dispatch in &turn.health_check_launches {
            self.register_launch(&dispatch.launch, registrar, &mut registrations)?;
        }
        for dispatch in &turn.service_launches {
            self.register_launch(&dispatch.launch, registrar, &mut registrations)?;
        }
        for dispatch in &turn.submitted_launches {
            self.register_submitted_launch(dispatch, registrar, &mut registrations)?;
        }
        Ok(registrations)
    }

    /// A submitted job's pipes, plus its output sink when the submitter
    /// attached one. The sink is adopted here and closed when the pipes go.
    pub fn register_submitted_launch<R>(
        &mut self,
        dispatch: &crate::supervisor::SupervisorSubmittedLaunchDispatch,
        registrar: &mut R,
        registrations: &mut Vec<RuntimeEventSource>,
    ) -> Result<(), RuntimeEventRegistrationError>
    where
        R: RuntimeEventRegistrar + ?Sized,
    {
        let before = registrations.len();
        self.register_launch(&dispatch.launch, registrar, registrations)?;
        let open_pipes = registrations.len() - before;
        if let Some(fd) = dispatch.output_sink_fd {
            let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
            if open_pipes == 0 || set_nonblocking(&fd).is_err() {
                // Nothing will ever be written: close it now rather than hold
                // a descriptor nobody feeds.
                return Ok(());
            }
            self.sinks.insert(
                dispatch.launch.job_id,
                super::OutputSink {
                    fd,
                    open_pipes,
                    dropped: 0,
                    drop_reported: false,
                },
            );
        }
        Ok(())
    }

    pub fn register_retained_launches<R>(
        &mut self,
        launches: &[LaunchCreatedJobDispatch],
        registrar: &mut R,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventRegistrationError>
    where
        R: RuntimeEventRegistrar + ?Sized,
    {
        let mut registrations = Vec::new();
        for launch in launches {
            self.register_launch(launch, registrar, &mut registrations)?;
        }
        Ok(registrations)
    }

    pub fn register_completed_launch<R>(
        &mut self,
        launch: &LaunchCreatedJobDispatch,
        registrar: &mut R,
    ) -> Result<Vec<RuntimeEventSource>, RuntimeEventRegistrationError>
    where
        R: RuntimeEventRegistrar + ?Sized,
    {
        let mut registrations = Vec::new();
        self.register_launch(launch, registrar, &mut registrations)?;
        Ok(registrations)
    }

    fn register_launch<R>(
        &mut self,
        launch: &LaunchCreatedJobDispatch,
        registrar: &mut R,
        registrations: &mut Vec<RuntimeEventSource>,
    ) -> Result<(), RuntimeEventRegistrationError>
    where
        R: RuntimeEventRegistrar + ?Sized,
    {
        let origin = origin_for_job(&launch.job_event);
        if let Some(fd) = launch.process.stdout_fd {
            self.register_pipe(
                fd,
                origin.clone(),
                LogStream::Stdout,
                &launch.job_event,
                registrar,
            )?;
            registrations.push(RuntimeEventSource::ServiceLogPipe { fd });
        }
        if let Some(fd) = launch.process.stderr_fd {
            self.register_pipe(fd, origin, LogStream::Stderr, &launch.job_event, registrar)?;
            registrations.push(RuntimeEventSource::ServiceLogPipe { fd });
        }
        Ok(())
    }

    pub(in crate::runtime::logging) fn register_pipe<R>(
        &mut self,
        fd: i32,
        origin: String,
        stream: LogStream,
        event: &JobEvent,
        registrar: &mut R,
    ) -> Result<(), RuntimeEventRegistrationError>
    where
        R: RuntimeEventRegistrar + ?Sized,
    {
        if self.pipes.contains_key(&fd) {
            return Ok(());
        }
        let source = RuntimeEventSource::ServiceLogPipe { fd };
        if let Err(error) = registrar.register_source(fd, source) {
            unsafe {
                libc::close(fd);
            }
            return Err(error);
        }
        let pipe = ServiceLogPipe::new(
            fd,
            origin,
            stream,
            Some(event.job_id),
            self.config.max_line_bytes,
        );
        self.pipes.insert(fd, pipe);
        Ok(())
    }
}

fn set_nonblocking(fd: &std::os::fd::OwnedFd) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
