#!/bin/sh
# TTYPath exclusivity, TTYPrecedence and the tty:released handover, on a real
# PID 1. Run with dist/release/drive.py --no-disk --share <dir>.
#
# Uses /dev/tty2 rather than /dev/console: the console is the terminal this
# script is being typed at, and taking it away mid-run would lose the report.
#
# What it proves, in order:
#   1. A second claimant of a held terminal is Skipped with tty_unavailable.
#   2. Stopping the holder starts the waiter, by itself.
#   3. A service with no tty:released trigger is not woken.
#
# REG_ASSUME_YES because `reg` prompts on the console, and the console is
# where the next line of this script is arriving from.
export REG_ASSUME_YES=1
OUT=/share/tty-arbitration.txt
say() { echo "$@" >> "$OUT"; }
# No filtering: the live image's /bin has no grep, and a report built out of
# one silently reads "<no status>" for every service (cost one boot).
state() { svctl status "$1" >> "$OUT" 2>&1; }

# drive.py has already mounted the 9p share at /share.
: > "$OUT"

say "== peinit =="
svctl --version >> "$OUT" 2>&1 || say "(svctl has no --version)"
say "== /dev/tty2 =="
ls -l /dev/tty2 >> "$OUT" 2>&1

cat > /run/tty-probe.reg <<'REG'
{
 "keys": [
  {"path": "Machine\\System\\Services\\ttyprobe-hi",
   "values": [
    {"name": "ImagePath", "type": "sz", "data": "/bin/sleep"},
    {"name": "Arguments", "type": "multi", "data": ["900"]},
    {"name": "Identity", "type": "sz", "data": "SYSTEM"},
    {"name": "TTYPath", "type": "sz", "data": "/dev/tty2"},
    {"name": "TTYPrecedence", "type": "dword", "data": 100},
    {"name": "Triggers", "type": "multi", "data": ["tty:released"]},
    {"name": "Readiness", "type": "dword", "data": 1},
    {"name": "RestartPolicy", "type": "dword", "data": 0},
    {"name": "DisplayName", "type": "sz", "data": "TTY probe (high precedence)"}
   ]},
  {"path": "Machine\\System\\Services\\ttyprobe-lo",
   "values": [
    {"name": "ImagePath", "type": "sz", "data": "/bin/sleep"},
    {"name": "Arguments", "type": "multi", "data": ["900"]},
    {"name": "Identity", "type": "sz", "data": "SYSTEM"},
    {"name": "TTYPath", "type": "sz", "data": "/dev/tty2"},
    {"name": "TTYPrecedence", "type": "dword", "data": 10},
    {"name": "Triggers", "type": "multi", "data": ["tty:released"]},
    {"name": "Readiness", "type": "dword", "data": 1},
    {"name": "RestartPolicy", "type": "dword", "data": 0},
    {"name": "DisplayName", "type": "sz", "data": "TTY probe (low precedence)"}
   ]},
  {"path": "Machine\\System\\Services\\ttyprobe-deaf",
   "values": [
    {"name": "ImagePath", "type": "sz", "data": "/bin/sleep"},
    {"name": "Arguments", "type": "multi", "data": ["900"]},
    {"name": "Identity", "type": "sz", "data": "SYSTEM"},
    {"name": "TTYPath", "type": "sz", "data": "/dev/tty2"},
    {"name": "Readiness", "type": "dword", "data": 1},
    {"name": "RestartPolicy", "type": "dword", "data": 0},
    {"name": "DisplayName", "type": "sz", "data": "TTY probe (no handover trigger)"}
   ]}
 ]
}
REG

say "== apply definitions =="
reg apply /run/tty-probe.reg >> "$OUT" 2>&1
svctl reload-config >> "$OUT" 2>&1

say "== a definition that names no terminal must refuse these fields =="
cat > /run/tty-bad.reg <<'REG'
{"keys": [
 {"path": "Machine\\System\\Services\\ttyprobe-bad",
  "values": [
   {"name": "ImagePath", "type": "sz", "data": "/bin/sleep"},
   {"name": "Triggers", "type": "multi", "data": ["tty:released"]}
  ]}
]}
REG
reg apply /run/tty-bad.reg >> "$OUT" 2>&1
svctl reload-config
say "-- ttyprobe-bad (expect a validation failure, not a running service):"
state ttyprobe-bad
reg del Machine\\System\\Services\\ttyprobe-bad >> "$OUT" 2>&1
svctl reload-config >> "$OUT" 2>&1

say ""
say "== 1. the holder takes the terminal =="
svctl start ttyprobe-hi >> "$OUT" 2>&1
say "-- ttyprobe-hi (expect active):"
state ttyprobe-hi

say ""
say "== 2. a second claimant is skipped, not started =="
svctl start ttyprobe-lo >> "$OUT" 2>&1
say "-- ttyprobe-lo (expect skipped / tty_unavailable):"
svctl status ttyprobe-lo >> "$OUT" 2>&1
say "-- ttyprobe-deaf (expect skipped too):"
svctl start ttyprobe-deaf >> "$OUT" 2>&1
svctl status ttyprobe-deaf >> "$OUT" 2>&1

say ""
say "== 3. stopping the holder hands the terminal on =="
svctl stop ttyprobe-hi >> "$OUT" 2>&1
sleep 2
say "-- ttyprobe-hi (expect inactive):"
state ttyprobe-hi
say "-- ttyprobe-lo (expect active: it asked to be woken):"
state ttyprobe-lo
say "-- ttyprobe-deaf (expect still skipped: it did not ask):"
state ttyprobe-deaf

say ""
say "== full status =="
svctl status ttyprobe-hi >> "$OUT" 2>&1
svctl status ttyprobe-lo >> "$OUT" 2>&1
svctl status ttyprobe-deaf >> "$OUT" 2>&1

say ""
say "== boot health (this build is a new PID 1) =="
svctl list >> "$OUT" 2>&1
sync
