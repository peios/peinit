# Building Peinit

Use pekit to build PID 1, its service control command, and its C client
library, and to run their tests. Pekit builds each crate on its own, so a
change rebuilds only the crates it reaches. The supported development host
is x86-64 Linux.

## Before you start

You need pekit and Git. Everything else the build and tests use is pinned
and fetched by pekit on the first build: the Bootlin C toolchain, Rust
1.98.1 with Clippy, Python 3.14, libclang, busybox, the time zone database,
and the crates `Cargo.lock` selects. The first build needs network access;
later builds find what they need in pekit's store, and every step runs
without network access in a sandbox that holds only its declared inputs.

The header check in `tools/check` runs on the host. It needs cbindgen
0.29.2 and a C and C++ compiler:

```sh
cargo +1.98.1 install cbindgen --version 0.29.2 --locked
```

The build files use the gcslib libraries named in `workspace.gcsl` and the
build files, and the libraries of peios-rs and the Peios SDK. `pekit lock`
pins their commits in `pekit.lock`.

## Build

```sh
pekit build peinit2 svctl libpeinit
```

| Target | Output |
|---|---|
| `peinit2` | PID 1 and service manager |
| `svctl` | Service control command |
| `libpeinit` | Public C client library, `libpeinit.so.0` by its shared object name (SONAME) |
| `guest_tests` | The unit tests with the Peios features, for a Peios guest |

The programs and the library are built with the `peios-boundary` and
`peios-registry` features, which make them Peios's. Pekit prints where each
output is, under `pekit-out/<target>/`. They load `libpeios.so.0` by its
SONAME and carry no runtime search path. Do not run `peinit2` as the host
init process.

## Test

```sh
pekit test
```

| Test | Check |
|---|---|
| `portable` | The unit tests without the Peios features, which run on Linux |
| `library` | The library exports exactly the functions `abi/peinit-abi.h` declares, and a C consumer calls it without contacting PID 1 |
| `doctests` | The documentation tests, as `cargo test --doc` runs them |
| `clippy` | Clippy's checks of every target with both Peios features, with every warning denied |

The portable tests run shell scripts with busybox's `/bin/sh`, and compute
calendar times with the pinned time zone database. Their Unix-socket
fixtures live under the sandbox's `/tmp`.

`guest_tests` builds the test program with both Peios features. It needs
the Peios kernel interfaces, so pekit builds it but does not run it; run it
in a disposable Peios VM.

A test whose inputs did not change is found in the store and not run
again.

For the full contributor check, install the pinned PSC tools into a
directory outside the repository, then run the shared entry point:

```sh
python3 tools/prepare-style-tools.py "$HOME/.cache/peinit-style"
export PEIOS_STYLE_BIN="$HOME/.cache/peinit-style/bin"
tools/check
```

`tools/check` checks owned source formatting and lint with Ruff,
ShellCheck, shfmt, clang-format, Prettier, and rustfmt. It then runs
`tools/verify-abi.sh`, which checks the hand-written public headers against
the Rust ABI snapshot that cbindgen generates, and runs `pekit test`. The
tool versions and hashes are recorded in `tools/style-tools.lock.json`.
GitHub's workflow runs the style checks alone; builds and tests run with
pekit.

## How the build is planned

`build.gcsl` plans the package with the gcslib rust library's `workspace`:
Cargo works out the build, unit by unit, and pekit compiles each unit as a
step. Planning reads the manifest, `Cargo.lock`, and which files exist,
never the sources, so editing a source plans nothing again.

Keep Rust dependency versions in `Cargo.toml` and `Cargo.lock`, and update
the lockfile deliberately with Cargo. `peios-sys` generates its bindings from
the SDK's headers with bindgen and links libpeios, so it, and every crate
that depends on it, is given what peios-rs's library says it needs,
`crate_settings()`: libpeios and its headers.

libpeios and its headers are built from the Peios SDK's own files, through
the SDK's GCSL library, which peios-rs's library imports. The commits
`pekit.lock` pins say which SDK and which peios-rs; the rust library fails a
build whose `Cargo.lock` takes peios-rs's crates at another commit than
`pekit.lock` pins its library at. authd's `auth-core` takes the same
peios-rs commit, since a build holds one `peios-sys`.

## Not built with pekit yet

Packages and builds with the native Peios toolchain, which hardens
programs with control-flow enforcement technology (CET), are not yet built
with pekit. The package definitions and native build settings from the
Bazel build are kept outside this repository until they are.

The native userspace shares its host kernel. Passing its portable tests and
ELF checks does not qualify Peios registry calls, tokens, PID 1, boot,
shutdown, or terminal arbitration. Run those on a disposable Peios VM and
record the image, kernel, source revision, test results, and shutdown
outcome.
