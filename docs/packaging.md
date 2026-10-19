# Packaging Peinit

Build the full package family with `./bazel build //:packages`. The output
`bazel-bin/packages.ppk/` contains canonical manifests and compressed,
content-addressed blobs. Packaging uses the vendored PPK writer; it does not
install packages, apply registry seeds, or sign a release.

The seven package identities retain separate payloads:

| Package | Contents |
| --- | --- |
| `dev.peios.peinit` | PID 1, its manual, registry and event references |
| `dev.peios.peinit-svctl` | Control client, shutdown aliases and manuals |
| `dev.peios.libpeinit` | `libpeinit.so.0` |
| `dev.peios.libpeinit-devel` | Public headers, linker symlink and pkg-config file |
| `dev.peios.peinit-debuginfo` | Build-ID-indexed debug information |
| `dev.peios.peinit-debugsource` | Declared source inputs for debugging |
| `dev.peios.peinit-source` | Maintained source tree, excluding archives and output |

The PID 1 package depends on `org.iana.tzdata` for calendar timers using
named timezones. The isolated test environment needs that data too.

Bazel stages binaries and their declared dependency inputs. The package checks
verify file boundaries, licenses, aliases, SONAME, split debug information,
blob hashes, PIE, full RELRO, a non-executable stack, and the absence of
build-directory runtime library paths. Native builds additionally require
CET properties and packed relative relocations.

The library retains unwinding support because its C entry points contain
Rust panics with `catch_unwind`. Packaging does not substitute `panic=abort`.

Development packages use the host Linux ABI. Native packages use the selected
Peios toolchain and SDK. Build records identify that distinction and leave
release readiness and signing false: a successful package build is not a
qualified boot image or an authorized release.
