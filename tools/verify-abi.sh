#!/usr/bin/env bash
#
# verify-abi.sh - prove the hand-written <peinit/*.h> headers match the Rust ABI.
#
# The shipping headers are hand-written so they can carry ownership, threading,
# and boundary documentation. The generated abi/peinit-abi.h snapshot is the
# drift gate for the Rust extern "C" surface.

set -euo pipefail
cd "$(dirname "$0")/.."

SNAPSHOT=abi/peinit-abi.h
REQUIRED_CBINDGEN_VERSION=0.29.2
INC=(-I include)
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

check_cbindgen() {
  command -v cbindgen >/dev/null 2>&1 || fail "cbindgen not on PATH"
  local version
  version="$(cbindgen --version | awk '{print $2}')"
  [[ "$version" == "$REQUIRED_CBINDGEN_VERSION" ]] \
    || fail "cbindgen $REQUIRED_CBINDGEN_VERSION required, found ${version:-unknown}"
}

run_cbindgen() {
  check_cbindgen
  cbindgen --config cbindgen.toml --lang c -o "$1" src/capi/mod.rs 2>/dev/null
}

norm_fns() {
  grep -hE 'peinit_[a-z_]+ *\(' "$1" \
    | sed -E 's#^/\*[^*]*\*/ ##; s/^extern //;
              s/\bunsigned int\b/uint32_t/g;
              s/ +/ /g; s/ ;$/;/; s/ $//' \
    | sort -u
}

norm_typedefs() {
  grep -hE '^typedef void peinit_[a-z_]+_t;' "$1" | sort -u
}

norm_constants() {
  grep -hE '^#define PEINIT_[A-Z_]+ -?[0-9]+' "$@" | sed -E 's/ +/ /g' | sort -u
}

run_cbindgen "$TMP/gen.h"
diff -u "$SNAPSHOT" "$TMP/gen.h" \
  || fail "$SNAPSHOT is stale - regenerate it with cbindgen"
echo "ok 1/5: ABI snapshot is up to date"

printf '#include <peinit.h>\n'                   > "$TMP/hand.c"
printf '#include "%s/%s"\n' "$PWD" "$SNAPSHOT" > "$TMP/snap.c"

gcc "${INC[@]}" -fsyntax-only -xc   "$SNAPSHOT" || fail "snapshot does not compile as C"
g++ "${INC[@]}" -fsyntax-only -xc++ "$SNAPSHOT" || fail "snapshot does not compile as C++"
gcc "${INC[@]}" -fsyntax-only -xc   "$TMP/hand.c" || fail "hand-written headers do not compile as C"
g++ "${INC[@]}" -fsyntax-only -xc++ "$TMP/hand.c" || fail "hand-written headers do not compile as C++"
echo "ok 2/5: generated and hand-written headers compile as C and C++"

gcc "${INC[@]}" -aux-info "$TMP/hand.aux" -c -o /dev/null "$TMP/hand.c"
gcc "${INC[@]}" -aux-info "$TMP/snap.aux" -c -o /dev/null "$TMP/snap.c"
norm_fns "$TMP/hand.aux" > "$TMP/hand.fns"
norm_fns "$TMP/snap.aux" > "$TMP/snap.fns"
diff "$TMP/hand.fns" "$TMP/snap.fns" \
  || fail "function signature mismatch ('<' hand-written, '>' Rust snapshot)"
echo "ok 3/5: $(wc -l < "$TMP/hand.fns") function signatures match"

norm_typedefs include/peinit/base.h > "$TMP/hand.types"
norm_typedefs "$SNAPSHOT" > "$TMP/snap.types"
diff "$TMP/hand.types" "$TMP/snap.types" \
  || fail "opaque typedef mismatch ('<' hand-written, '>' Rust snapshot)"
echo "ok 4/5: opaque handle typedefs match"

norm_constants include/peinit/base.h include/peinit/control.h > "$TMP/hand.constants"
norm_constants "$SNAPSHOT" > "$TMP/snap.constants"
diff "$TMP/hand.constants" "$TMP/snap.constants" \
  || fail "PEINIT_* constant mismatch ('<' hand-written, '>' Rust snapshot)"
echo "ok 5/5: PEINIT_* constants match"

echo
echo "ABI VERIFIED: <peinit/*.h> matches the Rust C ABI snapshot."
