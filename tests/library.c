/* SPDX-License-Identifier: MIT */
#include <peinit.h>

#include <assert.h>
#include <stddef.h>
#include <string.h>

int main(int argc, char **argv) {
    assert(argc == 2);
    assert(peinit_abi_version() == 0);
    assert(strcmp(peinit_library_version(), argv[1]) == 0);
    assert(strcmp(peinit_status_name(PEINIT_ERR_INVALID_ARGUMENT), "invalid_argument") == 0);

    peinit_client_t *client = NULL;
    peinit_error_t *error = NULL;
    /* Invalid arguments fail before attempting a connection. */
    assert(peinit_client_connect_path(NULL, &client, &error) == PEINIT_ERR_INVALID_ARGUMENT);
    assert(client == NULL);
    assert(error != NULL);
    assert(peinit_error_code(error) == PEINIT_ERR_INVALID_ARGUMENT);
    assert(peinit_error_message(error) != NULL);
    peinit_error_free(error);
    peinit_client_free(NULL);
    peinit_error_free(NULL);
    assert(peinit_error_code(NULL) == PEINIT_OK);
    return 0;
}
