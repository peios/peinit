#!/bin/sh
# Select the compiler without allowing the Debian reference environment to
# weaken the native dependency contract.  Peios must provide its released Rust
# package; apt's older rustc is used only to materialize Cargo's vendor tree.

set -eu

case "${PEKIT_DEPENDENCY_PROVIDER:-}" in
  peipkg|"")
    # The Peipkg solver owns the declared >= 1.98.1 version contract.  Do not
    # second-guess its dependency-root selection here; the tool-presence gate
    # in build.main still fails closed before any source is compiled.
    command -v rustc >/dev/null
    command -v cargo >/dev/null
    ;;

  apt)
    reference_rust=$PEKIT_VENDOR_OUT/reference-rust
    reference_tools=$PEKIT_VENDOR_OUT/reference-tools
    test -x "$reference_rust/bin/rustc"
    test -x "$reference_rust/bin/cargo"
    test -x "$reference_tools/cbindgen"
    export PATH=$reference_rust/bin:$reference_tools:$PATH
    test "$(rustc --version | awk '{print $2}')" = 1.98.1
    test "$(cargo --version | awk '{print $2}')" = 1.98.1
    test "$(cbindgen --version | awk '{print $2}')" = 0.29.2
    ;;

  *)
    echo "peinit: unknown dependency provider: ${PEKIT_DEPENDENCY_PROVIDER:-unset}" >&2
    exit 1
    ;;
esac
