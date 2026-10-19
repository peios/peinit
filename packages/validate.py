"""Check peinit's staged service-manager payload without launching applications or applying seeds."""

import gzip
import os
import re
import subprocess
import tomllib
from pathlib import Path

BINARIES = ("peinit", "svctl")
LIBDIR = "usr/lib/x86_64-linux-peios"


def require(condition: bool, message: str) -> None:
    """Keep package gates effective when Python assertions are disabled."""
    if not condition:
        raise ValueError(message)


def readelf(path: Path, option: str) -> str:
    """Read one ELF view, tolerating binary CRC bytes in the debug-link section."""
    return subprocess.check_output(
        ["readelf", option, str(path)],
        text=True,
        errors="replace",
        env=os.environ | {"LC_ALL": "C"},
    )


def build_id(notes: str, path: Path) -> str:
    """Extract the single GNU build identity required by split debugging."""
    matches = re.findall(r"Build ID: ([0-9a-f]+)", notes)
    require(len(matches) == 1 and len(matches[0]) >= 3, f"{path}: missing or ambiguous build ID")
    return matches[0]


def validate_elf(stage: Path, binary: str, *, native: bool) -> None:
    """Require linked hardening and the matching shared-family split debug payload."""
    library = binary == "libpeinit"
    elf = stage / (
        LIBDIR + "/libpeinit.so.0"
        if library
        else ("usr/bin/peinit2" if binary == "peinit" else "usr/bin/svctl")
    )
    require(elf.is_file() and os.access(elf, os.X_OK), f"{elf}: missing executable")
    header, program, dynamic = (readelf(elf, option) for option in ("-h", "-lW", "-dW"))
    notes, sections = readelf(elf, "-n"), readelf(elf, "-SW")
    require(bool(re.search(r"Type:\s+DYN\b", header)), f"{elf}: not position independent")
    require("GNU_RELRO" in program and "BIND_NOW" in dynamic, f"{elf}: missing full RELRO")
    stack = [line for line in program.splitlines() if "GNU_STACK" in line]
    require(len(stack) == 1 and bool(re.search(r"\sRW\s", stack[0])), f"{elf}: executable stack")
    if not library:
        require(bool(re.search(r"FLAGS_1.*\bPIE\b", dynamic)), f"{elf}: missing PIE flag")
    if binary in {"peinit", "svctl", "libpeinit"}:
        require("Shared library: [libpeios.so.0]" in dynamic, f"{elf}: missing libpeios linkage")
    if library:
        require("Library soname: [libpeinit.so.0]" in dynamic, f"{elf}: incorrect client SONAME")
    require(not re.search(r"\((RPATH|RUNPATH|TEXTREL)\)", dynamic), f"{elf}: unsafe dynamic tag")
    require(".debug_info" not in sections, f"{elf}: unsplit debug information")
    if native:
        require("IBT" in notes and "SHSTK" in notes, f"{elf}: missing native CET properties")
        require(bool(re.search(r"\(RELR\)", dynamic)), f"{elf}: missing native packed relocations")
    identity = build_id(notes, elf)
    debug = stage / f"debug/.build-id/{identity[:2]}/{identity[2:]}.debug"
    require(debug.is_file() and debug.stat().st_size > 0, f"{elf}: missing split debug file")
    require(".debug_info" in readelf(debug, "-SW"), f"{debug}: missing debug information")
    require(build_id(readelf(debug, "-n"), debug) == identity, f"{debug}: build ID mismatch")
    require(debug.name in readelf(elf, "--string-dump=.gnu_debuglink"), f"{elf}: wrong debug link")
    forbidden = {str(Path.cwd().resolve()), str(Path.cwd().resolve()).split("/execroot/")[0]}
    for path in (elf, debug):
        contents = path.read_bytes()
        for prefix in forbidden:
            require(len(prefix) <= 1 or prefix.encode() not in contents, f"{path}: leaked {prefix}")


def validate_data(stage: Path) -> None:
    """Validate headers, command aliases and compressed manuals without service control."""
    for name in ["peinit2", "svctl"]:
        with gzip.open(stage / f"usr/share/man/man8/{name}.8.gz", "rt") as stream:
            require(".TH " in stream.read(), f"missing manual: {name}")
    for alias in ["reboot", "poweroff", "halt", "shutdown"]:
        require((stage / "usr/bin" / alias).readlink() == Path("svctl"), f"wrong alias: {alias}")
        require(
            (stage / "usr/share/man/man8" / (alias + ".8.gz")).readlink() == Path("svctl.8.gz"),
            f"wrong manual alias: {alias}",
        )
    require(
        (stage / LIBDIR / "libpeinit.so").readlink() == Path("libpeinit.so.0"), "wrong linker name"
    )
    for path in Path("include").rglob("*.h"):
        require((stage / "usr" / path).read_bytes() == path.read_bytes(), f"changed header: {path}")


def validate(stage: Path, *, native: bool, sdk: Path) -> None:
    """Check payloads without invoking PID 1, service commands, or registry writes."""
    require(sdk.is_dir(), "missing declared SDK")
    for binary in (*BINARIES, "libpeinit"):
        validate_elf(stage, binary, native=native)
    validate_data(stage)
    environment = os.environ | {
        "LD_LIBRARY_PATH": str((stage / LIBDIR).resolve()) + ":" + str((sdk / "lib").resolve())
    }
    result = subprocess.run(
        [stage / "usr/bin/svctl", "--version"],
        capture_output=True,
        text=True,
        env=environment,
        check=True,
    )
    version = tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"]
    require(result.stdout.strip() == f"svctl {version}", "wrong client version")
