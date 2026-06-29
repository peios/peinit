/* SPDX-License-Identifier: MIT */
/*
 * <peinit/notify.h> - service-to-Peinit notification helpers.
 *
 * These helpers send Peinit's systemd-compatible notify datagrams. The default
 * entry points read NOTIFY_SOCKET from the process environment, which Peinit
 * sets during service pre-exec. An unset NOTIFY_SOCKET is reported as
 * PEINIT_ERR_UNAVAILABLE.
 *
 * The explicit-path peinit_notify_send_to() entry point exists for tests and
 * specialist tools. Normal services should use the environment-driven helpers.
 */
#ifndef PEINIT_NOTIFY_H
#define PEINIT_NOTIFY_H

#include <stdint.h>

#include <peinit/base.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * peinit_notify_send - send a raw notify payload to $NOTIFY_SOCKET.
 *
 * message is a NUL-terminated UTF-8 notify payload containing one or more
 * KEY=VALUE lines. libpeinit validates the payload with the same parser Peinit
 * uses before sending it.
 */
int peinit_notify_send(const char *message, peinit_error_t **error_out);

/*
 * peinit_notify_send_to - send a raw notify payload to an explicit socket path.
 *
 * socket_path is a NUL-terminated filesystem path. message follows the same
 * rules as peinit_notify_send().
 */
int peinit_notify_send_to(const char *socket_path, const char *message, peinit_error_t **error_out);

/* Convenience wrappers around the common notify fields. */
int peinit_notify_ready(peinit_error_t **error_out);
int peinit_notify_reloading(peinit_error_t **error_out);
int peinit_notify_stopping(peinit_error_t **error_out);
int peinit_notify_watchdog(peinit_error_t **error_out);

/*
 * peinit_notify_status - send STATUS=<status>.
 *
 * status must not contain CR or LF. The returned status text is owned by Peinit
 * after the datagram is accepted; libpeinit does not retain it.
 */
int peinit_notify_status(const char *status, peinit_error_t **error_out);

/* Send WATCHDOG_USEC=<usec> or EXTEND_TIMEOUT_USEC=<usec>. */
int peinit_notify_watchdog_usec(unsigned long long usec, peinit_error_t **error_out);
int peinit_notify_extend_timeout_usec(unsigned long long usec, peinit_error_t **error_out);

#ifdef __cplusplus
}
#endif

#endif /* PEINIT_NOTIFY_H */
