#!/usr/bin/env python3
"""Exercise the real POSIX bootstrap with synthetic downloads and real SHA-256."""

import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest


REPOSITORY = Path(__file__).resolve().parents[2]
BOOTSTRAP = REPOSITORY / "scripts/bootstrap-desktop-check.sh"
ARTIFACT = "nanh-desktop-check-x86_64-unknown-linux-gnu"

CHECKER = '''#!/usr/bin/env python3
import json, os, sys
with open(os.environ["CHECKER_TEST_EXECUTIONS"], "a", encoding="utf-8") as log:
    log.write(json.dumps(sys.argv[1:]) + "\\n")
if sys.argv[1:] == ["--version"]:
    print(os.environ.get("CHECKER_TEST_VERSION", "nanh-desktop-check 0.1.0"))
    sys.exit(0)
sys.exit(int(os.environ.get("CHECKER_TEST_EXIT", "0")))
'''

CURL = '''#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys
arguments = sys.argv[1:]
for option in ["--proto", "--proto-redir"]:
    assert arguments[arguments.index(option) + 1] == "=https"
assert "--max-time" in arguments and "--max-filesize" in arguments
url = next(argument for argument in arguments if argument.startswith("https://"))
assert url.startswith("https://github.com/DavidLMS/nan-harness/releases/download/")
target = pathlib.Path(arguments[arguments.index("--output") + 1])
name = url.rsplit("/", 1)[1]
with open(os.environ["CHECKER_TEST_DOWNLOADS"], "a", encoding="utf-8") as log:
    log.write(json.dumps(url) + "\\n")
if name == os.environ.get("CHECKER_TEST_FAIL_DOWNLOAD"):
    target.write_bytes(b"partial synthetic download")
    sys.exit(22)
artifact = os.environ["CHECKER_TEST_ARTIFACT"]
binary = pathlib.Path(os.environ["CHECKER_TEST_BINARY"]).read_bytes()
if name == "release-version.txt":
    target.write_text("0.1.0\\n", encoding="utf-8")
elif name == "SHA256SUMS":
    checksum = hashlib.sha256(binary).hexdigest()
    mode = os.environ.get("CHECKER_TEST_CHECKSUM", "valid")
    if mode == "mismatch":
        checksum = "0" * 64
    line = checksum + "  " + artifact + "\\n"
    target.write_text(line * (2 if mode == "duplicate" else 1), encoding="utf-8")
elif name == artifact:
    target.write_bytes(binary)
else:
    raise AssertionError("unexpected download")
'''

UNAME = '''#!/bin/sh
case "$1" in
  -s) printf '%s\\n' "${CHECKER_TEST_SYSTEM:-Linux}" ;;
  -m) printf '%s\\n' "${CHECKER_TEST_ARCHITECTURE:-x86_64}" ;;
  *) exit 1 ;;
esac
'''


class BootstrapContracts(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="checker-bootstrap-contract-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.download_root = self.root / "downloads"
        self.download_root.mkdir()
        self.bin = self.root / "commands"
        self.bin.mkdir()
        self.executions = self.root / "executions.jsonl"
        self.downloads = self.root / "downloads.jsonl"
        self.checker = self.root / "synthetic-checker"
        self.checker.write_text(CHECKER, encoding="utf-8")
        for name, source in [("curl", CURL), ("uname", UNAME)]:
            path = self.bin / name
            path.write_text(source, encoding="utf-8")
            path.chmod(0o700)
        self.environment = dict(os.environ)
        self.environment.pop("NAN_DESKTOP_CHECK_VERSION", None)
        self.environment.pop("NAN_API_KEY", None)
        self.environment.update({
            "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
            "TMPDIR": str(self.download_root),
            "CHECKER_TEST_EXECUTIONS": str(self.executions),
            "CHECKER_TEST_DOWNLOADS": str(self.downloads),
            "CHECKER_TEST_ARTIFACT": ARTIFACT,
            "CHECKER_TEST_BINARY": str(self.checker),
        })

    def run_bootstrap(self, *arguments, **overrides):
        return subprocess.run(
            ["sh", str(BOOTSTRAP), *arguments],
            env={**self.environment, **overrides},
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
            timeout=15,
        )

    def calls(self, path):
        return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()] if path.exists() else []

    def assert_clean(self):
        self.assertEqual(list(self.download_root.iterdir()), [])
        self.assertEqual(self.checker.read_text(encoding="utf-8"), CHECKER)

    def test_passes_literal_arguments_and_exact_exit_then_cleans(self):
        arguments = ["--yes", "--app", "zed-desktop", "--model", "model with spaces", "--output", "report;literal.json"]
        result = self.run_bootstrap(*arguments, CHECKER_TEST_EXIT="17")
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertEqual(self.calls(self.executions), [["--version"], arguments])
        self.assert_clean()

    def test_ephemeral_retains_only_the_owned_private_download(self):
        result = self.run_bootstrap("--ephemeral", "--yes")
        self.assertEqual(result.returncode, 0, result.stderr)
        retained = list(self.download_root.iterdir())
        self.assertEqual(len(retained), 1)
        self.assertTrue(retained[0].name.startswith("nanh-desktop-check."))
        self.assertEqual(stat.S_IMODE(retained[0].stat().st_mode), 0o700)
        self.assertEqual((retained[0] / ARTIFACT).read_bytes(), self.checker.read_bytes())
        self.assertIn("Retained checker download", result.stderr)

    def test_bad_or_duplicate_checksums_never_execute(self):
        for mode in ["mismatch", "duplicate"]:
            with self.subTest(mode=mode):
                result = self.run_bootstrap("--yes", CHECKER_TEST_CHECKSUM=mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.calls(self.executions), [])
                self.assert_clean()

    def test_failed_download_removes_partial_owned_files(self):
        result = self.run_bootstrap("--yes", CHECKER_TEST_FAIL_DOWNLOAD=ARTIFACT)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No installed applications were changed", result.stderr)
        self.assertEqual(self.calls(self.executions), [])
        self.assert_clean()

    def test_unsupported_platform_never_downloads(self):
        result = self.run_bootstrap(CHECKER_TEST_SYSTEM="FreeBSD")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(self.downloads), [])
        self.assertEqual(self.calls(self.executions), [])
        self.assert_clean()

    def test_pinned_version_uses_only_immutable_release_assets(self):
        result = self.run_bootstrap("validate-report", "synthetic-report.json", NAN_DESKTOP_CHECK_VERSION="0.1.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        downloads = self.calls(self.downloads)
        self.assertEqual(len(downloads), 2)
        self.assertTrue(all("/desktop-check-v0.1.0/" in url for url in downloads))
        self.assertEqual(self.calls(self.executions)[-1], ["validate-report", "synthetic-report.json"])
        self.assert_clean()

    def test_wrong_binary_version_cannot_start_checks(self):
        result = self.run_bootstrap("--yes", CHECKER_TEST_VERSION="nanh-desktop-check 9.9.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(self.executions), [["--version"]])
        self.assert_clean()

    def test_checksum_fixture_is_real_sha256(self):
        result = self.run_bootstrap("--ephemeral")
        self.assertEqual(result.returncode, 0, result.stderr)
        retained = next(self.download_root.iterdir())
        expected = hashlib.sha256(self.checker.read_bytes()).hexdigest()
        self.assertEqual((retained / "SHA256SUMS").read_text(encoding="utf-8"), f"{expected}  {ARTIFACT}\n")


if __name__ == "__main__":
    unittest.main()
