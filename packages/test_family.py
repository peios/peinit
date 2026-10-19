"""Verify independent package boundaries and complete source/debug coverage."""

import json
import tomllib
import unittest
from pathlib import Path


class FamilyTests(unittest.TestCase):
    def test_family(self) -> None:
        store = Path("packages.store")
        index = json.loads((store / "index.json").read_text())
        packages = {p["name"].removeprefix("dev.peios."): p for p in index["packages"]}
        self.assertEqual(
            set(packages),
            {
                "peinit",
                "peinit-svctl",
                "libpeinit",
                "libpeinit-devel",
                "peinit-debuginfo",
                "peinit-debugsource",
                "peinit-source",
            },
        )
        files = {
            name: tomllib.loads((store / p["manifest"]).read_text())["files"]
            for name, p in packages.items()
        }
        self.assertFalse(index["release_ready"])
        self.assertTrue(files["peinit"]["usr/bin/peinit2"]["exec"])
        self.assertNotIn("usr/bin/svctl", files["peinit"])
        self.assertTrue(files["peinit-svctl"]["usr/bin/svctl"]["exec"])
        self.assertEqual(files["peinit-svctl"]["usr/bin/reboot"], {"symlink": "svctl"})
        self.assertIn("usr/lib/x86_64-linux-peios/libpeinit.so.0", files["libpeinit"])
        self.assertIn("usr/include/peinit.h", files["libpeinit-devel"])
        self.assertEqual(
            files["libpeinit-devel"]["usr/lib/x86_64-linux-peios/libpeinit.so"],
            {"symlink": "libpeinit.so.0"},
        )
        self.assertEqual(sum(p.endswith(".debug") for p in files["peinit-debuginfo"]), 3)
        for name, p in packages.items():
            self.assertEqual(p["version"], "0.0.12-1")
            self.assertFalse(p["release_ready"])
        for name in [
            "Cargo.lock",
            "MODULE.bazel",
            "vendor/BUILD.bazel",
            "src/lib.rs",
            "src/init/autorun_process.rs",
        ]:
            self.assertIn("usr/src/peinit/" + name, files["peinit-source"])
        self.assertFalse(
            any("/target/" in name or "/docs/archive/" in name for name in files["peinit-source"])
        )


if __name__ == "__main__":
    unittest.main()
