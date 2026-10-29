# Peinit

Peinit is the Peios PID 1 service manager. It boots the system, supervises
services and submitted jobs, and coordinates shutdown. This repository also
provides `svctl` and the shared C client library `libpeinit.so.0`.

The installed init executable remains `peinit2`; the boot handoff uses that
name. The Rust package name is `peinit2`, and its library crate is `peinit`.

- [Build and test](docs/building.md)
- [Coding conventions](docs/CODING_STYLE.md)
- [Learn documentation](docs/learn/)
- [Command manuals](docs/man/), [registry reference](docs/peinit.regman),
  and [event reference](docs/peinit.evman)
- [Public C headers](include/peinit.h)

Pekit builds and tests peinit; packages are not yet built with pekit. Cargo
remains available for editor integration. Portable tests run on
Linux; boot, identity, registry and shutdown qualification require a disposable
Peios VM. Do not run `peinit2` as the host init process or run VM service-control
tests on a development workstation.
