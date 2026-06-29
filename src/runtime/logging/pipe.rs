use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::ids::JobId;
use crate::logging::{LogLineAssembler, LogStream, ServiceLogRecord};

#[derive(Debug)]
pub(super) struct ServiceLogPipe {
    fd: OwnedFd,
    origin: String,
    stream: LogStream,
    job_id: Option<JobId>,
    lines: LogLineAssembler,
}

impl ServiceLogPipe {
    pub(super) fn new(
        fd: i32,
        origin: String,
        stream: LogStream,
        job_id: Option<JobId>,
        max_line_bytes: usize,
    ) -> Self {
        Self {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            origin,
            stream,
            job_id,
            lines: LogLineAssembler::new(max_line_bytes),
        }
    }

    pub(super) fn read_available(&mut self, timestamp_ns: u64, budget_bytes: usize) -> LogPipeRead {
        let mut records = Vec::new();
        let mut total_read = 0;
        let mut closed = false;
        let mut would_block = false;
        let mut buffer = [0_u8; 4096];

        while total_read < budget_bytes {
            let remaining = budget_bytes - total_read;
            let read_len = remaining.min(buffer.len());
            match read_fd(self.fd.as_raw_fd(), &mut buffer[..read_len]) {
                Ok(0) => {
                    closed = true;
                    if let Some(line) = self.lines.finish() {
                        records.push(self.record(line, timestamp_ns));
                    }
                    break;
                }
                Ok(read) => {
                    total_read += read;
                    for line in self.lines.push_bytes(&buffer[..read]) {
                        records.push(self.record(line, timestamp_ns));
                    }
                }
                Err(error) if is_interrupted(&error) => {}
                Err(error) if is_would_block(&error) => {
                    would_block = true;
                    break;
                }
                Err(_) => {
                    closed = true;
                    if let Some(line) = self.lines.finish() {
                        records.push(self.record(line, timestamp_ns));
                    }
                    break;
                }
            }
        }

        LogPipeRead {
            records,
            closed,
            would_block,
        }
    }

    fn record(&self, line: String, timestamp_ns: u64) -> ServiceLogRecord {
        ServiceLogRecord::new(
            self.origin.clone(),
            self.stream,
            line,
            timestamp_ns,
            self.job_id,
        )
    }
}

pub(super) struct LogPipeRead {
    pub(super) records: Vec<ServiceLogRecord>,
    pub(super) closed: bool,
    pub(super) would_block: bool,
}

fn read_fd(fd: i32, buffer: &mut [u8]) -> io::Result<usize> {
    let read = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    if read < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(read as usize)
    }
}

fn is_interrupted(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Interrupted
}

fn is_would_block(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK
    )
}
