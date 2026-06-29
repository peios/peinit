/* SPDX-License-Identifier: MIT */
/*
 * <peinit/base.h> - common libpeinit ABI types and error helpers.
 *
 * All handles are opaque and owned by libpeinit. Free them with the matching
 * peinit_*_free function; never use free(3) on a libpeinit handle or string.
 *
 * Most functions return PEINIT_OK on client-side success. A Peinit control
 * command rejected by PID 1 is still a successful transport operation: inspect
 * the returned peinit_response_t with peinit_response_is_ok(),
 * peinit_response_error_code(), and peinit_response_error_message().
 *
 * Nonzero return values describe local client failures: invalid arguments, I/O
 * errors, malformed protocol responses, or a missing external surface such as
 * NOTIFY_SOCKET. When an error_out parameter is supplied, the function stores a
 * peinit_error_t with a stable numeric code and a diagnostic message.
 */
#ifndef PEINIT_BASE_H
#define PEINIT_BASE_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PEINIT_OK 0
#define PEINIT_ERR_INVALID_ARGUMENT -1
#define PEINIT_ERR_IO -2
#define PEINIT_ERR_PROTOCOL -3
#define PEINIT_ERR_UNAVAILABLE -4
#define PEINIT_ERR_NO_MEMORY -5
#define PEINIT_ERR_INTERNAL -6

typedef void peinit_client_t;
typedef void peinit_response_t;
typedef void peinit_error_t;

/*
 * peinit_abi_version - return the libpeinit C ABI version.
 *
 * ABI version 0 is the initial development ABI. A future incompatible ABI break
 * increments this value and the shared-object soname.
 */
unsigned int peinit_abi_version(void);

/*
 * peinit_library_version - return the libpeinit implementation version string.
 *
 * The returned pointer is static storage owned by libpeinit.
 */
const char *peinit_library_version(void);

/*
 * peinit_status_name - return a static symbolic name for a PEINIT_* status.
 *
 * Unknown values return "unknown". The returned pointer is static storage owned
 * by libpeinit.
 */
const char *peinit_status_name(int status);

/* Free an error object returned through an error_out parameter. NULL is valid. */
void peinit_error_free(peinit_error_t *error);

/* Return a PEINIT_* status code for @error, or PEINIT_OK for NULL. */
int peinit_error_code(const peinit_error_t *error);

/*
 * Return the diagnostic message for @error, or NULL for NULL.
 *
 * The returned pointer is borrowed from @error and becomes invalid when @error
 * is freed.
 */
const char *peinit_error_message(const peinit_error_t *error);

/*
 * Free a heap string returned by a future libpeinit API. NULL is valid.
 *
 * The current v0 surface exposes borrowed strings from response/error handles,
 * but this helper is part of the base ABI so future string-returning functions
 * can use one ownership rule.
 */
void peinit_string_free(char *value);

#ifdef __cplusplus
}
#endif

#endif /* PEINIT_BASE_H */
