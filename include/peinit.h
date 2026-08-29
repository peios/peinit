/* SPDX-License-Identifier: MIT */
/*
 * <peinit.h> - libpeinit umbrella header.
 *
 * libpeinit is Peinit's public client/integration ABI. It does not embed PID 1
 * or expose supervisor internals; it wraps Peinit's stable process-facing
 * surfaces:
 *
 *   - <peinit/notify.h>: service readiness, reload, status, and watchdog
 *     notifications sent to the NOTIFY_SOCKET Peinit placed in the service
 *     environment.
 *   - <peinit/control.h>: synchronous client helpers for the Peinit control
 *     socket at /run/services/peinit/control.sock, including the job
 *     query and stop commands.
 *   - <peinit/jobs.h>: synchronous client helpers for the Peinit jobs
 *     socket at /run/services/peinit/jobs.sock: submitting a job and
 *     managing it as its submitter.
 *
 * Registry editing, eventd queries, auth/token operations, and
 * service-definition parsing are intentionally outside this ABI. Those
 * belong to their own subsystem libraries unless a future Peinit protocol
 * explicitly pulls them in.
 */
#ifndef PEINIT_H
#define PEINIT_H

#include <peinit/base.h>
#include <peinit/control.h>
#include <peinit/jobs.h>
#include <peinit/notify.h>

#endif /* PEINIT_H */
