//! The jobs socket (PSPU §7) for C callers: a blocking `peinit_jobs_t`
//! wrapping [`JobsClient`], answering with the same `peinit_response_t` the
//! control socket uses plus the process handle a `submit` carries.

use std::os::fd::{AsRawFd, BorrowedFd};

use libc::{c_char, c_int};
use serde_json::Value;

use crate::jobs::client::{JobsClient, JobsClientError, JobsResponse};
use crate::jobs::socket::{JOBS_SOCKET_PATH, MAX_JOBS_MESSAGE_DESCRIPTORS};

use super::control::{ResponseObject, clear_response_out};
use super::error::{ErrorDetail, ffi_status, utf8_arg};
use super::types::{peinit_error_t, peinit_jobs_t, peinit_response_t};

#[unsafe(no_mangle)]
pub extern "C" fn peinit_default_jobs_socket_path() -> *const c_char {
    c"/run/services/peinit/jobs.sock".as_ptr()
}

#[allow(dead_code)]
fn _assert_default_jobs_path_matches_runtime() {
    assert_eq!(JOBS_SOCKET_PATH, "/run/services/peinit/jobs.sock");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_connect_default(
    out: *mut *mut peinit_jobs_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    unsafe { peinit_jobs_connect_path(peinit_default_jobs_socket_path(), out, error_out) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_connect_path(
    path: *const c_char,
    out: *mut *mut peinit_jobs_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        if out.is_null() {
            return Err(ErrorDetail::invalid_argument("jobs out parameter is NULL"));
        }
        unsafe {
            *out = std::ptr::null_mut();
        }
        let path = utf8_arg(path, "path")?;
        let client = JobsClient::connect_path(path).map_err(ErrorDetail::from)?;
        unsafe {
            *out = Box::into_raw(Box::new(client)).cast::<peinit_jobs_t>();
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_free(jobs: *mut peinit_jobs_t) {
    if !jobs.is_null() {
        unsafe {
            drop(Box::from_raw(jobs.cast::<JobsClient>()));
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_fd(jobs: *const peinit_jobs_t) -> c_int {
    if jobs.is_null() {
        return -1;
    }
    unsafe { &*jobs.cast::<JobsClient>() }.as_fd().as_raw_fd()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_job_submit(
    jobs: *mut peinit_jobs_t,
    definition_json: *const c_char,
    token_fd: c_int,
    fds: *const c_int,
    fd_count: usize,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        let client = jobs_mut(jobs)?;
        let definition_json = utf8_arg(definition_json, "definition_json")?;
        let definition: Value = serde_json::from_str(definition_json)
            .map_err(|_| ErrorDetail::invalid_argument("definition_json is not valid JSON"))?;
        if !definition.is_object() {
            return Err(ErrorDetail::invalid_argument(
                "definition_json must be a JSON object",
            ));
        }
        if fd_count > 0 && fds.is_null() {
            return Err(ErrorDetail::invalid_argument(
                "fds is NULL with fd_count > 0",
            ));
        }
        if fd_count > MAX_JOBS_MESSAGE_DESCRIPTORS {
            return Err(ErrorDetail::invalid_argument(format!(
                "fd_count {fd_count} exceeds the {MAX_JOBS_MESSAGE_DESCRIPTORS} the manager accepts"
            )));
        }
        let raw_fds = if fd_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(fds, fd_count) }
        };
        if raw_fds.iter().any(|fd| *fd < 0) {
            return Err(ErrorDetail::invalid_argument(
                "fds contains a negative descriptor",
            ));
        }
        if token_fd < -1 {
            return Err(ErrorDetail::invalid_argument(
                "token_fd must be -1 or a descriptor",
            ));
        }
        // Borrowed for the call only: the caller keeps every descriptor.
        let borrowed: Vec<BorrowedFd<'_>> = raw_fds
            .iter()
            .map(|fd| unsafe { BorrowedFd::borrow_raw(*fd) })
            .collect();
        let token = (token_fd >= 0).then(|| unsafe { BorrowedFd::borrow_raw(token_fd) });
        let response = client
            .submit(definition, token, &borrowed)
            .map_err(ErrorDetail::from)?;
        response_from_jobs(response)?.store(response_out);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_status(
    jobs: *mut peinit_jobs_t,
    job_id: *const c_char,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    jobs_command(jobs, job_id, response_out, error_out, |client, job_id| {
        client.status(job_id)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_wait(
    jobs: *mut peinit_jobs_t,
    job_id: *const c_char,
    for_ready: bool,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    jobs_command(jobs, job_id, response_out, error_out, |client, job_id| {
        client.wait(job_id, for_ready)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_stop(
    jobs: *mut peinit_jobs_t,
    job_id: *const c_char,
    wait: bool,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    jobs_command(jobs, job_id, response_out, error_out, |client, job_id| {
        client.stop(job_id, wait)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_signal(
    jobs: *mut peinit_jobs_t,
    job_id: *const c_char,
    signal: c_int,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    jobs_command(jobs, job_id, response_out, error_out, |client, job_id| {
        client.signal(job_id, signal)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn peinit_jobs_raw_json(
    jobs: *mut peinit_jobs_t,
    request_json: *const c_char,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
) -> c_int {
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        let client = jobs_mut(jobs)?;
        let request_json = utf8_arg(request_json, "request_json")?;
        let request: Value = serde_json::from_str(request_json)
            .map_err(|_| ErrorDetail::invalid_argument("request_json is not valid JSON"))?;
        if !request.is_object() {
            return Err(ErrorDetail::invalid_argument(
                "request_json must be a JSON object",
            ));
        }
        let response = client
            .request(&request, None, &[])
            .map_err(ErrorDetail::from)?;
        response_from_jobs(response)?.store(response_out);
        Ok(())
    })
}

fn jobs_command<F>(
    jobs: *mut peinit_jobs_t,
    job_id: *const c_char,
    response_out: *mut *mut peinit_response_t,
    error_out: *mut *mut peinit_error_t,
    send: F,
) -> c_int
where
    F: FnOnce(&mut JobsClient, &str) -> Result<JobsResponse, JobsClientError>,
{
    ffi_status(error_out, || {
        clear_response_out(response_out)?;
        let client = jobs_mut(jobs)?;
        let job_id = utf8_arg(job_id, "job_id")?;
        let response = send(client, job_id).map_err(ErrorDetail::from)?;
        response_from_jobs(response)?.store(response_out);
        Ok(())
    })
}

fn response_from_jobs(response: JobsResponse) -> Result<ResponseObject, ErrorDetail> {
    ResponseObject::new(
        &response.raw_json,
        if response.ok { "ok" } else { "error" },
        response.error_code.map(|code| code.as_str()),
        response.error_message.as_deref(),
        response.pidfd,
    )
}

fn jobs_mut<'a>(jobs: *mut peinit_jobs_t) -> Result<&'a mut JobsClient, ErrorDetail> {
    if jobs.is_null() {
        return Err(ErrorDetail::invalid_argument("jobs is NULL"));
    }
    Ok(unsafe { &mut *jobs.cast::<JobsClient>() })
}

impl From<JobsClientError> for ErrorDetail {
    fn from(error: JobsClientError) -> Self {
        match error {
            JobsClientError::Io(message) => Self::io(message),
            JobsClientError::Protocol(message) => Self::protocol(message),
        }
    }
}
