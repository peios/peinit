/* SPDX-License-Identifier: MIT */
/*
 * <peinit/jobs.h> - Peinit jobs socket client helpers.
 *
 * The jobs socket (PSPU §7) is where a process submits a job — a program
 * Peinit runs under an identity the submitter chooses, supervised like a
 * service's job — and manages the jobs it submitted. It is a SOCK_SEQPACKET
 * socket carrying one JSON object per record; a token attached to the submit
 * record (by the kernel, through KACS) becomes the job's primary token, and
 * descriptors attached with SCM_RIGHTS become the job's inherited
 * descriptors and its output sink.
 *
 * A peinit_jobs_t is a blocking client. Calls are not internally
 * synchronized; use one client per thread or serialize access externally.
 * Responses are the same peinit_response_t the control socket returns; the
 * job view is under "job".
 *
 * Who may submit is who may connect: the socket's file security descriptor
 * decides, not Peinit. Management of a job (status, wait, stop, signal) is
 * governed by the job's own security descriptor, which by default grants its
 * submitter, SYSTEM and Administrators everything.
 */
#ifndef PEINIT_JOBS_H
#define PEINIT_JOBS_H

#include <stdbool.h>
#include <stddef.h>

#include <peinit/base.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * peinit_default_jobs_socket_path - return "/run/services/peinit/jobs.sock".
 *
 * The returned pointer is static storage owned by libpeinit.
 */
const char *peinit_default_jobs_socket_path(void);

/*
 * peinit_jobs_connect_default - connect to the default Peinit jobs socket.
 *
 * On success, stores a new client in *out. On failure, stores NULL in *out
 * and, if error_out is non-NULL, stores a peinit_error_t describing the local
 * failure. Connecting is the access check: a caller without FILE_WRITE_DATA
 * on the socket is refused here.
 */
int peinit_jobs_connect_default(peinit_jobs_t **out, peinit_error_t **error_out);

/* Connect to a specific jobs socket path (tests and non-default deployments). */
int peinit_jobs_connect_path(const char *path, peinit_jobs_t **out, peinit_error_t **error_out);

/* Close and free a jobs client. NULL is valid. */
void peinit_jobs_free(peinit_jobs_t *jobs);

/*
 * peinit_jobs_fd - return the client's socket descriptor, or -1 for NULL.
 *
 * Borrowed: the client owns it. It is exposed so that a submitter can set
 * the socket's impersonation level with libpeios before a submit that must
 * attach a token at Delegation level; no other use is intended.
 */
int peinit_jobs_fd(const peinit_jobs_t *jobs);

/*
 * peinit_job_submit - submit a job (PSPU §7.6).
 *
 * definition_json is one JSON object with the job definition: "image_path"
 * (required, absolute), and optionally "arguments", "environment",
 * "working_directory", "description", "timeout", "stop_timeout",
 * "readiness", "readiness_timeout", "success_exit_codes", "descriptors",
 * "output" and "security_descriptor". A "command" field is overwritten.
 *
 * token_fd is a KACS token descriptor the kernel attaches to the record as
 * the job's identity, or -1 to run the job as this process's own primary
 * token. fds are fd_count descriptors passed with SCM_RIGHTS, in the order
 * "descriptors" names them, with the output sink last when "output" is true.
 * Every descriptor is borrowed for the call; the caller keeps them.
 *
 * The call blocks until the job leaves "created": the response is the job
 * view, and for a running job also carries its pidfd — take it with
 * peinit_response_take_pidfd. A job that failed to start is an OK response
 * whose view is terminal, not a transport error.
 */
int peinit_job_submit(peinit_jobs_t *jobs,
		      const char *definition_json,
		      int token_fd,
		      const int *fds,
		      size_t fd_count,
		      peinit_response_t **response_out,
		      peinit_error_t **error_out);

/* The job view now (JOB_QUERY). */
int peinit_jobs_status(peinit_jobs_t *jobs,
		       const char *job_id,
		       peinit_response_t **response_out,
		       peinit_error_t **error_out);

/*
 * Block until the job is terminal, or with for_ready until it has sent
 * READY=1 (or is terminal, whichever first); the response is the view then.
 * Requires JOB_QUERY.
 */
int peinit_jobs_wait(peinit_jobs_t *jobs,
		     const char *job_id,
		     bool for_ready,
		     peinit_response_t **response_out,
		     peinit_error_t **error_out);

/*
 * Ask Peinit to stop the job: the termination signal now, SIGKILL to its
 * cgroup after stop_timeout. With wait true the call blocks until the job is
 * terminal. Requires JOB_STOP.
 */
int peinit_jobs_stop(peinit_jobs_t *jobs,
		     const char *job_id,
		     bool wait,
		     peinit_response_t **response_out,
		     peinit_error_t **error_out);

/* Send one signal to the job's main process. Requires JOB_SIGNAL. */
int peinit_jobs_signal(peinit_jobs_t *jobs,
		       const char *job_id,
		       int signal,
		       peinit_response_t **response_out,
		       peinit_error_t **error_out);

/*
 * peinit_jobs_raw_json - send one JSON record and read one response.
 *
 * request_json must be one JSON object naming a "command". No token or
 * descriptors are attached; use peinit_job_submit for a submit that needs
 * them.
 */
int peinit_jobs_raw_json(peinit_jobs_t *jobs,
			 const char *request_json,
			 peinit_response_t **response_out,
			 peinit_error_t **error_out);

#ifdef __cplusplus
}
#endif

#endif /* PEINIT_JOBS_H */
