#!/usr/bin/env python3
"""Synthetic build/session contracts; never start an actual window manager."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "native_build", ROOT / "crates/nan-harness-desktop-check/native/build.py"
)
BUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILD)


class DesktopSessionTests(unittest.TestCase):
    def test_native_dependencies_do_not_fetch_sw_packages(self):
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(sys, "argv", ["build.py", "--output", directory]), \
                patch.object(BUILD, "download"), patch.object(BUILD, "unpack"), \
                patch.object(BUILD, "build") as build:
            BUILD.main()
            for call in build.call_args_list[:2]:
                self.assertIn("-DSW_BUILD=OFF", call.args[-1])

    def test_native_projects_share_the_explicit_static_runtime_policy(self):
        with patch.object(BUILD.subprocess, "run") as run:
            BUILD.build("cmake", Path("source"), Path("build"), Path("prefix"), [])
            configure = run.call_args_list[0].args[0]
            self.assertIn("-DCMAKE_POLICY_DEFAULT_CMP0091=NEW", configure)
            self.assertIn("-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded", configure)

    def test_macos_preparation_refuses_a_personal_session(self):
        environment = dict(os.environ, GITHUB_ACTIONS="false")
        result = subprocess.run(
            ["bash", str(ROOT / "canary/actions/prepare-desktop-macos.sh")],
            env=environment, capture_output=True, timeout=5,
        )
        self.assertNotEqual(result.returncode, 0)

    def test_x11_waits_for_readiness_and_preserves_checker_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            commands = {
                "openbox": '#!/bin/sh\n[ -z "${NAN_API_KEY+x}" ] || exit 90\nexec sleep 30\n',
                "xprop": '#!/bin/sh\necho "_NET_SUPPORTING_WM_CHECK(WINDOW): window id # 0x123"\n',
                "checker": '#!/bin/sh\n[ "$NAN_API_KEY" = synthetic ] || exit 91\nexit 23\n',
            }
            for name, content in commands.items():
                path = root / name
                path.write_text(content)
                path.chmod(0o700)
            environment = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}", NAN_API_KEY="synthetic")
            result = subprocess.run(
                ["bash", str(ROOT / "scripts/run-desktop-check-x11.sh"), str(root / "checker")],
                env=environment, capture_output=True, timeout=5,
            )
            self.assertEqual(result.returncode, 23)


if __name__ == "__main__":
    unittest.main()
