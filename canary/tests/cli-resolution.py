#!/usr/bin/env python3
"""Offline contracts for closed official-version resolver diagnostics."""

import importlib.util
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError, URLError
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
    def test_official_json_authenticates_only_exact_github_api_origin(self):
        class Response:
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def read(self, _limit): return b'{"tag_name":"v1.2.3"}'

        seen = []
        class Opener:
            def open(self, request, timeout):
                seen.append((request.full_url, request.get_header("Authorization"), timeout))
                return Response()

        urls = [
            "https://api.github.com/repos/block/goose/releases/latest",
            "https://api.github.com:444/repos/block/goose/releases/latest",
            "https://api.github.com@evil.example/repos/block/goose/releases/latest",
            "http://api.github.com/repos/block/goose/releases/latest",
            "https://api.github.com.evil.example/repos/block/goose/releases/latest",
        ]
        with patch.dict(suite.os.environ, {"GITHUB_TOKEN": "test-token"}), \
                patch.object(suite, "build_opener", return_value=Opener()):
            for url in urls:
                suite._official_json(url)
        self.assertEqual(seen[0][1], "Bearer test-token")
        self.assertTrue(all(auth is None for _, auth, _ in seen[1:]))
        self.assertTrue(all(timeout == 20 for _, _, timeout in seen))

    def test_official_json_without_token_remains_unauthenticated(self):
        class Response:
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def read(self, _limit): return b'{"tag_name":"v1.2.3"}'

        seen = []
        class Opener:
            def open(self, request, timeout):
                seen.append(request.get_header("Authorization"))
                return Response()

        with patch.dict(suite.os.environ, {}, clear=True), \
                patch.object(suite, "build_opener", return_value=Opener()):
            suite._official_json("https://api.github.com/repos/block/goose/releases/latest")
        self.assertEqual(seen, [None])

    def test_official_json_uses_no_redirect_handler_and_token_is_not_forwarded(self):
        class Response:
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def read(self, _limit): return b'{"tag_name":"v1.2.3"}'

        class Opener:
            def __init__(self, handlers): self.handlers = handlers
            def open(self, request, timeout): return Response()

        with patch.dict(suite.os.environ, {"GITHUB_TOKEN": "test-token"}), \
                patch.object(suite, "build_opener", side_effect=lambda *handlers: Opener(handlers)) as build:
            suite._official_json("https://api.github.com/repos/block/goose/releases/latest")
        handlers = build.call_args.args
        self.assertIn(suite._NoRedirect, handlers)
        self.assertIsNone(suite._NoRedirect().redirect_request(
            None, None, 302, "Found", {}, "https://evil.example"))

    def test_child_stage_environments_never_receive_github_token(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"harnesses": [], "unresolved": [{
                "harness": "goose", "system": "macos", "architecture": "aarch64",
                "source": "github:block/goose", "package": "", "model": "qwen3.6",
                "diagnostic": {"category": "http", "httpStatus": 403},
            }]}))
            argv = ["cli-suite.py", "--harnesses", "goose", "--mode", "deterministic",
                    "--trigger", "manual", "--tag", "v0.1.6", "--model", "qwen3.6",
                    "--binary", str(root / "nan"), "--canary", str(root / "canary"),
                    "--directory", str(root / "cells"), "--output", str(root / "reports"),
                    "--run-id", "run-1", "--system", "macos", "--architecture", "aarch64",
                    "--source-kind", "branch", "--source-sha", "b" * 40,
                    "--nan-version", "0.1.6", "--manifest", str(manifest)]
            with patch.dict(suite.os.environ, {"GITHUB_TOKEN": "test-token"}), \
                    patch.object(suite.subprocess, "run", return_value=type("Result", (), {"returncode": 0})()) as run, \
                    patch.object(sys, "argv", argv):
                self.assertEqual(suite.main(), 0)
            self.assertNotIn("GITHUB_TOKEN", run.call_args.kwargs["env"])

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
            state = json.loads(path.read_text())
            state.update({"tier": "deterministic", "scenario": "hosted-clean-install-deterministic-and-live-tool",
                          "harness": {"id": "goose", "version": "unknown"},
                          "environment": {"operatingSystem": "macos", "architecture": "aarch64"}})
            path.write_text(json.dumps(state))
            self.assertTrue(suite._annotate_resolution_report(
                path, "goose", {"category": "http", "httpStatus": 403}))
            report = json.loads(path.read_text())
            self.assertEqual(report["failure"]["code"], "resolve-http-403")
            self.assertEqual(len(report["failure"]["fingerprint"]), 64)
            expected = "|".join(("goose", "unknown", "macos", "aarch64", "deterministic",
                                  "hosted-clean-install-deterministic-and-live-tool", "Infrastructure",
                                  "resolve-official-version", "resolve-http-403"))
            self.assertEqual(report["failure"]["fingerprint"], hashlib.sha256(expected.encode()).hexdigest())
            self.assertNotIn("secret", path.read_text())

    def test_malformed_or_unrelated_report_is_preserved(self):
        base = {
            "outcome": "infrastructure-failure", "tier": "deterministic",
            "scenario": "hosted-clean-install-deterministic-and-live-tool",
            "harness": {"id": "goose", "version": "unknown"},
            "environment": {"operatingSystem": "macos", "architecture": "aarch64"},
            "failure": {"class": "infrastructure", "phase": "resolve-official-version",
                        "fingerprint": "d" * 64},
        }
        cases = [
            ("tier", None),
            ("failure", {"class": "harness", "phase": "resolve-official-version",
                          "fingerprint": "d" * 64}),
            ("failure", {"class": "infrastructure", "phase": "install-package",
                          "fingerprint": "d" * 64}),
            ("harness", {"id": "other", "version": "unknown"}),
        ]
        for field, value in cases:
            with self.subTest(field=field):
                with tempfile.TemporaryDirectory() as temporary:
                    path = Path(temporary) / "report.json"
                    state = json.loads(json.dumps(base))
                    if field == "tier":
                        del state[field]
                    else:
                        state[field] = value
                    original = json.dumps(state, sort_keys=True) + "\n"
                    path.write_text(original)
                    self.assertFalse(suite._annotate_resolution_report(
                        path, "goose", {"category": "timeout"}))
                    self.assertEqual(path.read_text(), original)

    def test_unresolved_manifest_flows_to_final_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "nan"
            canary = root / "canary"
            binary.write_bytes(b"binary")
            canary.write_bytes(b"canary")
            manifest = root / "manifest.json"
            manifest.write_text(json.dumps({"harnesses": [], "unresolved": [{
                "harness": "goose", "system": "macos", "architecture": "aarch64",
                "source": "github:block/goose", "package": "", "model": "qwen3.6",
                "diagnostic": {"category": "http", "httpStatus": 404},
            }]}))
            output = root / "reports"
            output.mkdir()
            (output / "macos-aarch64-goose.json").write_text(json.dumps({
                "schemaVersion": 2, "runId": "run-1", "cellId": "macos-goose-manual",
                "specSha256": "a" * 64, "trigger": "manual", "tier": "deterministic",
                "scenario": "hosted-clean-install-deterministic-and-live-tool",
                "startedAt": "2026-09-13T00:00:00Z", "completedAt": "2026-09-13T00:00:01Z",
                "durationMilliseconds": 1000, "nanHarness": {"version": "0.1.6",
                "source": "commit:" + "b" * 40, "sha256": "c" * 64},
                "environment": {"operatingSystem": "macos", "architecture": "aarch64",
                "image": "github-hosted", "profile": "clean-macos", "runtimes": []},
                "harness": {"id": "goose", "version": "unknown"}, "checks": [{
                "name": "resolve-official-version", "status": "failed",
                "durationMilliseconds": 1, "attempts": 1}], "outcome": "infrastructure-failure",
                "failure": {"class": "infrastructure", "phase": "resolve-official-version",
                "summary": "Hosted check did not complete successfully.", "fingerprint": "d" * 64},
            }))
            argv = ["cli-suite.py", "--harnesses", "goose", "--mode", "deterministic",
                    "--trigger", "manual", "--tag", "v0.1.6", "--model", "qwen3.6",
                    "--binary", str(binary), "--canary", str(canary), "--directory", str(root / "cells"),
                    "--output", str(output), "--run-id", "run-1", "--system", "macos",
                    "--architecture", "aarch64", "--source-kind", "branch", "--source-sha", "b" * 40,
                    "--nan-version", "0.1.6", "--manifest", str(manifest)]
            with patch.object(suite.sys, "argv", argv), patch.object(
                    suite.subprocess, "run", return_value=type("Result", (), {"returncode": 1})()):
                self.assertEqual(suite.main(), 1)
            report = json.loads((output / "macos-aarch64-goose.json").read_text())
            self.assertEqual(report["failure"]["code"], "resolve-http-404")
            self.assertEqual(report["failure"]["phase"], "resolve-official-version")
            self.assertNotIn("body", report["failure"])

    def test_wrapped_network_failures_remain_closed(self):
        errors = [
            (URLError(socket.timeout("secret")), "timeout"),
            (URLError(socket.gaierror("secret")), "dns"),
            (URLError(ssl.SSLError("secret")), "tls"),
        ]
        for error, category in errors:
            with self.subTest(category=category):
                self.assertEqual(self.resolve_error(error).diagnostic, {"category": category})


if __name__ == "__main__":
    unittest.main()
