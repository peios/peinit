---
title: Client Library
description: The shared libpeinit interface, version reporting, handle ownership,
  and the distinction between transport errors and rejected commands.
---

`libpeinit.so.0` provides the C interface for service control, job submission,
and service notifications. Include `<peinit.h>` from the development package
and link with `pkg-config --cflags --libs peinit`. The public headers describe
each function's arguments, ownership, and threading requirements.

## Versions

`peinit_library_version()` returns the package implementation version from
build metadata. For source version 0.0.12, it returns `"0.0.12"`. The string
uses static library storage; callers must not free it.

`peinit_abi_version()` returns `0`, matching the shared library's SONAME.
The implementation version and ABI version have separate meanings. Updating
the package does not itself change the ABI number.

## Handles and errors

Free each owned handle with its matching `peinit_*_free` function. Strings
returned by response and error accessors borrow storage from that handle;
use them before freeing the handle. Do not pass borrowed strings to
`peinit_string_free()` or the C allocator.

A successful client call means the exchange completed. The service manager can
still reject the command. Inspect `peinit_response_is_ok()` and the response
error accessors before treating the requested operation as successful.
Local failures return a `PEINIT_ERR_*` value and can provide an error handle.

## Permissions

The library does not grant authority. The control and jobs sockets check the
calling process and the requested operation. A client cannot select an
arbitrary service identity by changing a library argument.
