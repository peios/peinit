#!/usr/bin/env python3
"""Check the shared library exports and run a C consumer without contacting PID 1."""

import os
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path


def main() -> None:
    library, sdk = (Path(arg).resolve() for arg in sys.argv[1:])
    header = Path("abi/peinit-abi.h").read_text()
    expected = set(re.findall(r"\b(peinit_[a-z_]+)\s*\(", header))
    symbols = subprocess.check_output(["nm", "-D", "--defined-only", library], text=True)
    observed = {line.split()[-1] for line in symbols.splitlines()}
    if observed != expected:
        raise SystemExit(
            f"export mismatch: missing={expected - observed}, extra={observed - expected}"
        )
    version = tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"]
    with tempfile.TemporaryDirectory(prefix="peinit-abi-") as temporary:
        work = Path(temporary)
        (work / "libpeinit.so.0").symlink_to(library)
        (work / "libpeios.so.0").symlink_to(sdk)
        program = work / "consumer"
        subprocess.run(
            [
                "cc",
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Iinclude",
                "tests/library.c",
                str(library),
                str(sdk),
                f"-Wl,-rpath-link,{work}",
                "-o",
                str(program),
            ],
            check=True,
        )
        subprocess.run(
            [program, version],
            env=os.environ | {"LD_LIBRARY_PATH": str(work)},
            timeout=10,
            check=True,
        )
    print(f"Verified {len(expected)} exports, library {version}, and ABI 0")


if __name__ == "__main__":
    main()
