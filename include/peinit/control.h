/* SPDX-License-Identifier: MIT */
/*
 * <peinit/control.h> - Peinit control socket client helpers.
 *
 * The control socket speaks newline-delimited JSON. libpeinit keeps that wire
 * format visible by returning each response's raw JSON while also exposing the
 * common response envelope. This avoids freezing every service-status field into
 * C struct layout before those higher-level consumers exist.
 *
 * A peinit_client_t is a blocking Unix-stream client. Calls are not internally
 * synchronized; use one client per thread or serialize access externally.
 */
#ifndef PEINIT_CONTROL_H
#define PEINIT_CONTROL_H

#include <stdbool.h>

#include <peinit/base.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PEINIT_SHUTDOWN_POWEROFF 0
#define PEINIT_SHUTDOWN_REBOOT 1
#define PEINIT_SHUTDOWN_HALT 2

/*
 * peinit_default_control_socket_path - return "/run/services/peinit/control.sock".
 *
 * The returned pointer is static storage owned by libpeinit.
 */
const char *peinit_default_control_socket_path(void);

/*
 * peinit_client_connect_default - connect to the default Peinit control socket.
 *
 * On success, stores a new client in *out. On failure, stores NULL in *out and,
 * if error_out is non-NULL, stores a peinit_error_t describing the local failure.
 */
int peinit_client_connect_default(peinit_client_t **out, peinit_error_t **error_out);

/*
 * peinit_client_connect_path - connect to a specific Peinit control socket path.
 *
 * This is primarily for tests, recovery tools, and non-default deployments. The
 * path is a NUL-terminated UTF-8 filesystem path.
 */
int peinit_client_connect_path(const char *path, peinit_client_t **out, peinit_error_t **error_out);

/* Close and free a control client. NULL is valid. */
void peinit_client_free(peinit_client_t *client);

/*
 * peinit_control_raw_json - send a single JSON control request.
 *
 * request_json must be one JSON object without embedded CR/LF. libpeinit writes
 * the newline frame terminator, waits for one newline-delimited JSON response,
 * and stores it in *response_out.
 */
int peinit_control_raw_json(peinit_client_t *client,
			    const char *request_json,
			    peinit_response_t **response_out,
			    peinit_error_t **error_out);

int peinit_service_start(peinit_client_t *client,
			 const char *service,
			 bool wait,
			 peinit_response_t **response_out,
			 peinit_error_t **error_out);

int peinit_service_stop(peinit_client_t *client,
			const char *service,
			bool wait,
			peinit_response_t **response_out,
			peinit_error_t **error_out);

int peinit_service_restart(peinit_client_t *client,
			   const char *service,
			   bool wait,
			   peinit_response_t **response_out,
			   peinit_error_t **error_out);

int peinit_service_reload(peinit_client_t *client,
			  const char *service,
			  bool wait,
			  peinit_response_t **response_out,
			  peinit_error_t **error_out);

int peinit_service_reset(peinit_client_t *client,
			 const char *service,
			 peinit_response_t **response_out,
			 peinit_error_t **error_out);

int peinit_service_status(peinit_client_t *client,
			  const char *service,
			  peinit_response_t **response_out,
			  peinit_error_t **error_out);

int peinit_service_list(peinit_client_t *client,
			peinit_response_t **response_out,
			peinit_error_t **error_out);

int peinit_operation_status(peinit_client_t *client,
			    const char *operation_id,
			    peinit_response_t **response_out,
			    peinit_error_t **error_out);

int peinit_reload_config(peinit_client_t *client,
			 peinit_response_t **response_out,
			 peinit_error_t **error_out);

/*
 * Submitted jobs, as the control socket sees them (PSPU §4.14).
 *
 * peinit_job_status answers with the job view under "job"; the caller needs
 * JOB_QUERY on the job. peinit_job_list answers with "jobs", the views the
 * caller holds JOB_QUERY on that match the filter; filter_json is NULL or a
 * JSON object with any of "submitter", "identity" (SIDs), "logon_session"
 * (integer) and "state". peinit_job_stop asks Peinit to stop the job (the
 * caller needs JOB_STOP); with wait true the response is the terminal view.
 *
 * Submitting a job is not a control command: see <peinit/jobs.h>.
 */
int peinit_job_status(peinit_client_t *client,
		      const char *job_id,
		      peinit_response_t **response_out,
		      peinit_error_t **error_out);

int peinit_job_list(peinit_client_t *client,
		    const char *filter_json,
		    peinit_response_t **response_out,
		    peinit_error_t **error_out);

int peinit_job_stop(peinit_client_t *client,
		    const char *job_id,
		    bool wait,
		    peinit_response_t **response_out,
		    peinit_error_t **error_out);

/*
 * peinit_system_shutdown - request graceful poweroff/reboot/halt.
 *
 * shutdown_kind must be PEINIT_SHUTDOWN_POWEROFF, PEINIT_SHUTDOWN_REBOOT, or
 * PEINIT_SHUTDOWN_HALT. Tests should not call this against the host PID 1.
 */
int peinit_system_shutdown(peinit_client_t *client,
			   int shutdown_kind,
			   peinit_response_t **response_out,
			   peinit_error_t **error_out);

/* Free a response returned by a control command. NULL is valid. */
void peinit_response_free(peinit_response_t *response);

/*
 * Return the raw response JSON without the trailing newline.
 *
 * The returned pointer is borrowed from @response and becomes invalid when
 * @response is freed.
 */
const char *peinit_response_json(const peinit_response_t *response);

/* Return "ok" or "error", or NULL for an invalid response handle. */
const char *peinit_response_status(const peinit_response_t *response);

/* Return 1 for a Peinit {"status":"ok"} response, else 0. */
int peinit_response_is_ok(const peinit_response_t *response);

/*
 * Return the Peinit error code/message from a {"status":"error"} response.
 *
 * These accessors return NULL when the response is OK, when the server omitted
 * the field, or when @response is NULL. Returned pointers are borrowed from the
 * response object.
 */
const char *peinit_response_error_code(const peinit_response_t *response);
const char *peinit_response_error_message(const peinit_response_t *response);

/*
 * peinit_response_take_pidfd - take the process handle a response carried.
 *
 * A jobs-socket submit answered with a running job carries the job's pidfd
 * (PSPU §7.6). This returns it and transfers ownership to the caller, who
 * closes it; every later call, and any response that carried no handle,
 * returns -1. Freeing a response closes a handle that was not taken.
 */
int peinit_response_take_pidfd(peinit_response_t *response);

#ifdef __cplusplus
}
#endif

#endif /* PEINIT_CONTROL_H */
