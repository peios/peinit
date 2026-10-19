#!/usr/bin/env python3
"""Reject drift between Cargo, Bazel and the seven package definitions."""

import json
import re
import tomllib
from pathlib import Path

cargo = tomllib.loads(Path("Cargo.toml").read_text())
version = cargo["package"]["version"]
bazel_version = re.search(r'^VERSION = "([^"]+)"$', Path("build/version.bzl").read_text(), re.M)
if bazel_version is None or bazel_version[1] != version:
    raise SystemExit("Cargo and Bazel versions differ")
module = Path("MODULE.bazel").read_text()
for name in ("peios", "auth-core"):
    blocks = re.findall(r"crate\.spec\((.*?)\n\)", module, re.S)
    block = next(text for text in blocks if f'package = "{name}"' in text)
    for key in ("git", "rev"):
        if f'{key} = "{cargo["dependencies"][name][key]}"' not in block:
            raise SystemExit(f"Bazel {name} {key} differs from Cargo")
for path in Path("packages/definitions").glob("*.json"):
    package = json.loads(path.read_text())["package"]
    if package["version"] != version + "-1":
        raise SystemExit(f"package version differs: {path}")
print("Cargo, Bazel and package metadata agree")
