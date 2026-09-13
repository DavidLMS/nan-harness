#!/usr/bin/env python3
"""Offline contracts for closed official-version resolver diagnostics."""

import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from urllib.error import HTTPError
import socket
import ssl


ACTION_DIRECTORY = Path(__file__).resolve().parents[1] / "actions"
sys.path.insert(0, str(ACTION_DIRECTORY))
selection = type(sys)("selection")
selection.CLI_HARNESSES = (
    "claude-code", "codex", "opencode", "hermes", "pi", "omp", "prime-agent",
    "deepseek-harness", "openclaw", "cline", "qwen-code", "kimi-code", "aider",
    "goose", "fx",
)
selection.resolve_model = lambda requested="", configured=None: requested or configured or "qwen3.6"
sys.modules["selection"] = selection


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ACTION_DIRECTORY / filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


suite = load("cli_suite_resolution", "cli-suite.py")


class CliResolutionTests(unittest.TestCase):
    def resolve_error(self, error):
        def fetch_json(_url):
            raise error

        _, unresolved = suite.resolve_manifest(
            ["goose"], "macos", "aarch64", "qwen3.6", fetch_json=fetch_json)
        return unresolved[0]

    def test_closed_categories_and_http_status_are_preserved(self):
        cases = [
            (socket.timeout(), "timeout", None),
            (socket.gaierror("secret.example"), "dns", None),
            (ssl.SSLError("secret certificate"), "tls", None),
            *[
                (HTTPError("https://secret.example", status, "secret body", {}, io.BytesIO(b"secret")),
                 "http", status) for status in (403, 429, 404)
            ],
            (ValueError("secret json"), "unknown", None),
        ]
        for error, category, status in cases:
            with self.subTest(category=category):
                item = self.resolve_error(error)
                self.assertEqual(item.diagnostic, {"category": category, **(
                    {"httpStatus": status} if status is not None else {})})
                self.assertNotIn("secret", json.dumps(item.as_dict()))

    def test_json_tag_and_version_boundaries(self):
        def missing_tag(_url):
            return {}

        _, unresolved = suite.resolve_manifest(
            ["goose"], "macos", "aarch64", "qwen3.6", fetch_json=missing_tag)
        self.assertEqual(unresolved[0].diagnostic, {"category": "missing-tag"})

        def invalid_version(_url):
            return {"tag_name": "not-a-version"}

        _, unresolved = suite.resolve_manifest(
            ["goose"], "macos", "aarch64", "qwen3.6", fetch_json=invalid_version)
        self.assertEqual(unresolved[0].diagnostic, {"category": "invalid-version"})

        self.assertEqual(self.resolve_error(json.JSONDecodeError("secret", "{}", 0)).diagnostic,
                         {"category": "invalid-json"})

    def test_success_manifest_has_no_diagnostic_and_unresolved_is_strict(self):
        def success(_url):
            return {"tag_name": "v1.2.3"}

        resolved, unresolved = suite.resolve_manifest(
            ["goose"], "macos", "aarch64", "qwen3.6", fetch_json=success)
        self.assertEqual(unresolved, [])
        self.assertNotIn("diagnostic", resolved[0].as_dict())

        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "manifest.json"
            path.write_text(json.dumps({"harnesses": [], "unresolved": [{
                "harness": "goose", "system": "macos", "architecture": "aarch64",
                "source": "github:block/goose", "package": "", "model": "qwen3.6",
                "diagnostic": {"category": "http", "httpStatus": 403},
            }]}))
            _, loaded = suite._load_manifest(path, ["goose"], "macos", "aarch64", "qwen3.6")
            self.assertEqual(loaded[0].diagnostic["httpStatus"], 403)
            path.write_text(json.dumps({"harnesses": [], "unresolved": [{
                "harness": "goose", "system": "macos", "architecture": "aarch64",
                "source": "github:block/goose", "diagnostic": {"category": "http", "httpStatus": 403,
                "body": "secret"},
            }]}))
            with self.assertRaises(ValueError):
                suite._load_manifest(path, ["goose"], "macos", "aarch64", "qwen3.6")

    def test_report_annotation_is_closed_and_recomputes_fingerprint(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "report.json"
            path.write_text(json.dumps({"outcome": "infrastructure-failure", "failure": {
                "class": "infrastructure", "phase": "resolve-official-version",
                "summary": "safe", "fingerprint": "0" * 64,
            }}))
            suite._annotate_resolution_report(path, "goose", {"category": "http", "httpStatus": 403})
            report = json.loads(path.read_text())
            self.assertEqual(report["failure"]["code"], "resolve-http")
            self.assertEqual(len(report["failure"]["fingerprint"]), 64)
            self.assertNotIn("403", report["failure"]["code"])
            self.assertNotIn("secret", path.read_text())


if __name__ == "__main__":
    unittest.main()
