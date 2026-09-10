#!/bin/sh
# Establish the libpeios build interface without weakening the native rung.
#
# A Peios dependency root must contain the released development package. The
# Debian reference container cannot install Peios packages, so it builds the
# exact locked libpeios source into a private sysroot. The resulting DSO is a
# link/build fixture only; Peinit's portable tests run on Debian, while its
# Peios-boundary tests remain compile-only until the booted-system gate.

set -eu

case "${PEKIT_DEPENDENCY_PROVIDER:-}" in
  peipkg|"")
    test "$(pkg-config --modversion peios 2>/dev/null)" = 0.5.0 || {
      echo "peinit: dev.peios.libpeios-devel 0.5.0 is required" >&2
      exit 1
    }
    ;;

  apt)
    reference_source=$PEKIT_VENDOR_OUT/libpeios-reference
    reference_target=$PEKIT_OUT/libpeios-reference-target
    reference_root=$PEKIT_OUT/libpeios-reference-sysroot
    reference_lib=$reference_root/usr/lib/x86_64-linux-peios

    test -f "$reference_source/Cargo.lock"
    test -f "$reference_source/include/peios.h"
    test -f "$PEKIT_VENDOR_OUT/pkm-uapi/pkm/pkm.h"

    CARGO_TARGET_DIR=$reference_target \
      cargo build --release --locked --offline \
        --manifest-path "$reference_source/Cargo.toml"

    test -s "$reference_target/release/libpeios.so"
    test "$(readelf -dW "$reference_target/release/libpeios.so" |
      sed -n 's/.*Library soname: \[\([^]]*\)\].*/\1/p')" = libpeios.so.0

    mkdir -p "$reference_lib/pkgconfig" "$reference_root/usr/include"
    cp "$reference_target/release/libpeios.so" "$reference_lib/libpeios.so.0"
    ln -s libpeios.so.0 "$reference_lib/libpeios.so"
    cp -R "$reference_source/include/." "$reference_root/usr/include/"
    cp -R "$PEKIT_VENDOR_OUT/pkm-uapi/pkm" "$reference_root/usr/include/"
    sed -e 's|@prefix@|/usr|g' \
        -e 's|@libdir@|/usr/lib/x86_64-linux-peios|g' \
        -e 's|@version@|0.5.0|g' \
        "$reference_source/peios.pc.in" > "$reference_lib/pkgconfig/peios.pc"

    export PKG_CONFIG_PATH=$reference_lib/pkgconfig
    export PKG_CONFIG_SYSROOT_DIR=$reference_root
    export LD_LIBRARY_PATH=$reference_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
    test "$(pkg-config --modversion peios)" = 0.5.0
    ;;

  *)
    echo "peinit: unknown dependency provider: ${PEKIT_DEPENDENCY_PROVIDER:-unset}" >&2
    exit 1
    ;;
esac
