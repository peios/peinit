# Building Peinit

Use x86_64 Linux for development, with a C and C++ compiler, binutils,
libclang, Python 3.11 or later, pkg-config, gzip, timezone data (`tzdata`),
and cbindgen 0.29.2.
The wrapper verifies Bazel 8.4.2 before using it. Cargo checks use Rust 1.98.1.

## Development SDK

Set `PEIOS_SDK` to an absolute directory containing `include/peios.h`,
`include/pkm/`, and `lib/libpeios.so.0`. Build an SDK from explicit source
revisions with `tools/prepare-dev-sdk LIBPEIOS KERNEL_UAPI NEW_OUTPUT`.
The CI workflow records the revisions used for its development SDK.
The output directory must be new to prevent mixing SDK versions.

Build the init executable, control client, and shared library:

```sh
./bazel build //:artifacts
```

Run the portable Rust suite, ABI checks, metadata checks and packaging tests:

```sh
./bazel test //...
```

The ABI check needs cbindgen on `/usr/local/bin:/usr/bin:/bin`. It compares
all 52 exported C functions, compiles both header forms as C and C++, and runs
a C consumer that checks versions and invalid-argument handling without
contacting PID 1.

`//:portable` and `//:tests` omit the `peios-boundary` and `peios-registry`
features. Production artifacts enable both. `//:guest-tests` compiles the
full-feature test executable and carries the `manual` and `peios-kernel`
tags: ordinary host tests do not execute it.

## Style and additional checks

Prepare the pinned tools outside the repository, then run the combined check:

```sh
python3 tools/prepare-style-tools.py /tmp/peinit-style
PEIOS_STYLE_BIN=/tmp/peinit-style/bin tools/check
```

The check includes all-feature Clippy diagnostics. Cargo's default test graph
is portable; full-feature tests require the Peios kernel interfaces.
Use a short `TMPDIR`, such as `/tmp`, for Unix-socket fixtures.

## Native builds

Run inside an isolated Peios userspace containing its patched Rust compiler,
hardened standard library, C compiler, native SDK, and timezone data under
`/usr/share/zoneinfo`. Select `--config=peios`;
`PEIOS_RUSTC` can select the installed compiler. The same configuration cannot
make a Debian SDK or Debian Rust compiler produce a native Peios build.

Acquire dependencies with `tools/offline-build acquire SNAPSHOT` on the
networked development host. Supply that frozen snapshot to
`tools/offline-build build SNAPSHOT WORK` in the isolated userspace. `SNAPSHOT`
and `WORK` are absolute paths; `WORK` is writable and private to that build.
The helper rejects a changed module lock and disables dependency downloads.

The native userspace shares its host kernel. Passing its portable tests and
ELF checks does not qualify Peios registry calls, tokens, PID 1, boot,
shutdown, or terminal arbitration. Run those on a disposable Peios VM and
record the image, kernel, source revision, test results, and shutdown outcome.
