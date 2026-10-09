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
from unittest.mock import Mock, patch
from urllib.error import HTTPError, URLError
import socket
import ssl


ACTION_DIRECTORY = Path(__file__).resolve().parents[1] / "actions"
sys.path.insert(0, str(ACTION_DIRECTORY))
# The hosted platform table is the single source of truth, so these tests load the
# real selector and only pin model resolution to keep them environment-independent.
def _load_selection():
    spec = importlib.util.spec_from_file_location("selection", ACTION_DIRECTORY / "selection.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules["selection"] = module
    spec.loader.exec_module(module)
    return module


selection = _load_selection()
selection.resolve_model = lambda requested="", configured=None: requested or configured or "qwen3.6"


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ACTION_DIRECTORY / filename)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


suite = load("cli_suite_resolution", "cli-suite.py")


class CliResolutionTests(unittest.TestCase):
    def test_hermes_resolves_tag_version_when_packaging_is_a_placeholder(self):
        commit = "a" * 40
        fetch = Mock(side_effect=[{"tag_name": "v0.21.6"}, {"sha": commit}])
        document = Mock(return_value='[project]\nversion = "0.0.0"\n')
        resolved, unresolved = suite.resolve_manifest(["hermes"], "linux", "aarch64", "qwen3.6",
                                                     fetch_json=fetch, fetch_document=document)
        self.assertEqual(unresolved, [])
        self.assertEqual((resolved[0].version, resolved[0].ref), ("0.21.6", commit))
        document.assert_called_once_with("https://raw.githubusercontent.com/NousResearch/hermes-agent/"
                                         + commit + "/pyproject.toml")
        self.assertEqual(fetch.call_args.args[0],
                         "https://api.github.com/repos/NousResearch/hermes-agent/commits/v0.21.6")

    def test_hermes_never_resolves_a_placeholder_or_calver_as_the_product_version(self):
        for tag in ("v0.0.0", "v2026.9.11", "v0.21.6-rc.1", "v00.21.6"):
            with self.subTest(tag=tag):
                fetch = Mock(side_effect=[{"tag_name": tag}, {"sha": "a" * 40}])
                resolved, unresolved = suite.resolve_manifest(
                    ["hermes"], "windows", "x86_64", "qwen3.6", fetch_json=fetch,
                    fetch_document=lambda url: '[project]\nversion = "0.0.0"\n')
                self.assertEqual(resolved, [])
                self.assertEqual(unresolved[0].diagnostic["category"], "invalid-version")

    def test_zcode_freezes_main_and_reads_agent_version_from_that_commit(self):
        commit = "a" * 40
        fetch = Mock(return_value={"sha": commit})
        document = Mock(return_value='{"version":"0.16.9"}')
        resolved, unresolved = suite.resolve_manifest(["zcode"], "windows", "x86_64", "qwen3.6",
                                                     fetch_json=fetch, fetch_document=document)
        self.assertEqual(unresolved, [])
        self.assertEqual((resolved[0].version, resolved[0].ref), ("0.16.9", commit))
        fetch.assert_called_once_with("https://api.github.com/repos/zai-org/ZCode/commits/main")
        document.assert_called_once_with("https://raw.githubusercontent.com/zai-org/ZCode/" + commit
                                         + "/apps/zcode-cli/package.json")
        for value in ("main", "../private", "b" * 39):
            bad, unresolved = suite.resolve_manifest(["zcode"], "linux", "aarch64", "qwen3.6",
                                                     fetch_json=lambda url: {"sha": value},
                                                     fetch_document=document)
            self.assertEqual(bad, [])
            self.assertEqual(unresolved[0].harness, "zcode")

    def test_hermes_recovers_from_throttling_at_each_official_endpoint(self):
        commit = "a" * 40
        urls = [
            "https://api.github.com/repos/NousResearch/hermes-agent/releases/latest",
            "https://api.github.com/repos/NousResearch/hermes-agent/commits/v2026.9.11",
            "https://raw.githubusercontent.com/NousResearch/hermes-agent/" + commit + "/pyproject.toml",
        ]
        documents = [b'{"tag_name":"v2026.9.11"}',
                     json.dumps({"sha": commit}).encode(), b'[project]\nversion = "0.21.2"\n']
        for throttled_url in urls:
            with self.subTest(endpoint=throttled_url):
                seen = []
                failed_body = io.BytesIO(b"private upstream error")

                def open_request(request, timeout):
                    url = request.full_url
                    seen.append((url, request.get_header("Authorization"), timeout))
                    if url == throttled_url and sum(item[0] == url for item in seen) == 1:
                        raise HTTPError(url, 429, "private reason", {"Retry-After": "2"}, failed_body)
                    return io.BytesIO(documents[urls.index(url)])

                opener = Mock()
                opener.open.side_effect = open_request
                with patch.object(suite, "build_opener", return_value=opener), \
                        patch.object(suite.time, "sleep") as sleep, \
                        patch.dict(suite.os.environ, {"GITHUB_TOKEN": "test-token"}):
                    resolved, unresolved = suite.resolve_manifest(
                        ["hermes"], "linux", "aarch64", "qwen3.6")
                self.assertEqual(unresolved, [])
                self.assertEqual((resolved[0].version, resolved[0].ref), ("0.21.2", commit))
                self.assertEqual(len(seen), 4)
                self.assertTrue(failed_body.closed)
                sleep.assert_called_once_with(2)
                for url, auth, timeout in seen:
                    self.assertEqual(auth, None if url == urls[2] else "Bearer test-token")
                    self.assertEqual(timeout, 20)

    def test_persistent_throttling_is_bounded_and_preserves_safe_diagnostic(self):
        opener = Mock()
        opener.open.side_effect = [HTTPError("https://private.example", 429,
                                             "private reason", {}, io.BytesIO(b"private body"))
                                   for _ in range(3)]
        with patch.object(suite, "build_opener", return_value=opener), \
                patch.object(suite.time, "sleep") as sleep:
            resolved, unresolved = suite.resolve_manifest(
                ["hermes"], "linux", "aarch64", "qwen3.6")
        self.assertEqual(resolved, [])
        self.assertEqual(unresolved[0].diagnostic, {"category": "http", "httpStatus": 429})
        self.assertEqual(opener.open.call_count, 3)
        self.assertEqual([call.args[0] for call in sleep.call_args_list], [60, 120])
        self.assertNotIn("private", json.dumps(unresolved[0].as_dict()))

    def test_long_or_invalid_cooldowns_fail_without_retrying_early(self):
        for value in ("121", "garbage", "-1"):
            with self.subTest(retry_after=value):
                opener = Mock()
                opener.open.side_effect = HTTPError("https://example.com", 429, "limited",
                                                    {"Retry-After": value}, None)
                with patch.object(suite, "build_opener", return_value=opener), \
                        patch.object(suite.time, "sleep") as sleep:
                    with self.assertRaises(HTTPError):
                        suite._official_text("https://example.com/stable")
                self.assertEqual(opener.open.call_count, 1)
                sleep.assert_not_called()

    def test_cooldown_honors_http_dates_and_exhausted_rate_limit_reset(self):
        with patch.object(suite.time, "time", return_value=0):
            self.assertEqual(suite._rate_limit_delay(
                {"Retry-After": "Thu, 01 Jan 1970 00:01:30 GMT"}, 0), 90)
            self.assertEqual(suite._rate_limit_delay(
                {"Retry-After": "2", "X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "90"}, 0), 90)
            self.assertIsNone(suite._rate_limit_delay(
                {"X-RateLimit-Remaining": "0", "X-RateLimit-Reset": "3600"}, 0))

    def test_other_http_failures_are_not_retried(self):
        for status in (302, 401, 403, 404, 500):
            with self.subTest(status=status):
                opener = Mock()
                opener.open.side_effect = HTTPError("https://example.com", status, "failure", {}, None)
                with patch.object(suite, "build_opener", return_value=opener), \
                        patch.object(suite.time, "sleep") as sleep:
                    with self.assertRaises(HTTPError):
                        suite._official_json("https://example.com/metadata")
                self.assertEqual(opener.open.call_count, 1)
                sleep.assert_not_called()

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
            "https://api.github.com/repos/aaif-goose/goose/releases/latest",
            "https://api.github.com:444/repos/aaif-goose/goose/releases/latest",
            "https://api.github.com@evil.example/repos/aaif-goose/goose/releases/latest",
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
            suite._official_json("https://api.github.com/repos/aaif-goose/goose/releases/latest")
        self.assertEqual(seen, [None])

    def test_official_text_is_noncredential_and_bounded(self):
        class Response:
            def __enter__(self): return self
            def __exit__(self, *args): pass
            def read(self, limit):
                self.limit = limit
                return b"v1.2.3"

        seen = []
        response = Response()
        class Opener:
            def open(self, request, timeout):
                seen.append((request, timeout))
                return response

        with patch.dict(suite.os.environ, {"GITHUB_TOKEN": "must-not-be-used"}), \
                patch.object(suite, "build_opener", return_value=Opener()):
            self.assertEqual(suite._official_text("https://example.com/stable", limit=32), "v1.2.3")
        request, timeout = seen[0]
        self.assertIsNone(request.get_header("Authorization"))
        self.assertEqual(request.get_header("User-agent"), "nan-harness-cli-gate")
        self.assertEqual(timeout, 20)
        self.assertEqual(response.limit, 33)

    def test_kimi_uses_the_vendor_channel_on_every_platform(self):
        # Both platforms install the vendor's own Kimi CLI, so Windows resolves through the
        # same stable channel Unix does instead of a PyPI distribution.
        for system, architecture in (("windows", "x86_64"), ("linux", "aarch64")):
            seen = []

            def fetch_text(url):
                seen.append(url)
                return "0.43.0"

            resolved, unresolved = suite.resolve_manifest(
                ["kimi-code"], system, architecture, "qwen3.6", fetch_text=fetch_text)
            self.assertEqual(unresolved, [])
            self.assertEqual(resolved[0].version, "0.43.0")
            self.assertEqual(resolved[0].source, "https://cdn.kimi.com/kimi-code/latest")
            self.assertEqual(seen, ["https://cdn.kimi.com/kimi-code/latest"])

    def test_aider_uses_pypi_metadata_for_windows_pip_install(self):
        seen = []

        def fetch_json(url):
            seen.append(url)
            return {"info": {"version": "1.2.3"}}

        resolved, unresolved = suite.resolve_manifest(
            ["aider"], "windows", "x86_64", "qwen3.6", fetch_json=fetch_json)
        self.assertEqual(unresolved, [])
        self.assertEqual(resolved[0].version, "1.2.3")
        self.assertEqual(resolved[0].source, "pypi:aider-chat")
        self.assertEqual(seen, ["https://pypi.org/pypi/aider-chat/json"])

    def test_kimi_preserves_unix_stable_channel(self):
        seen = []

        def fetch_text(url):
            seen.append(url)
            return "0.43.0"

        resolved, unresolved = suite.resolve_manifest(
            ["kimi-code"], "linux", "aarch64", "qwen3.6", fetch_text=fetch_text)
        self.assertEqual(unresolved, [])
        self.assertEqual(resolved[0].version, "0.43.0")
        self.assertEqual(resolved[0].source, "https://cdn.kimi.com/kimi-code/latest")
        self.assertEqual(resolved[0].package, "")
        self.assertEqual(seen, ["https://cdn.kimi.com/kimi-code/latest"])

    def test_kimi_rejects_a_malformed_vendor_channel(self):
        for document in ("", "latest", "v", "1.2"):
            with self.subTest(document=document):
                resolved, unresolved = suite.resolve_manifest(
                    ["kimi-code"], "windows", "x86_64", "qwen3.6",
                    fetch_text=lambda _url, value=document: value)
                self.assertEqual(resolved, [])
                self.assertEqual(unresolved[0].diagnostic, {"category": "invalid-version"})

    def test_aider_rejects_missing_or_malformed_pypi_versions(self):
        for document in ({"info": {}}, {"info": {"version": None}}, {"info": {"version": "latest"}}):
            with self.subTest(document=document):
                resolved, unresolved = suite.resolve_manifest(
                    ["aider"], "windows", "x86_64", "qwen3.6",
                    fetch_json=lambda _url, value=document: value)
                self.assertEqual(resolved, [])
                self.assertEqual(unresolved[0].diagnostic, {"category": "invalid-version"})

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
            suite._official_json("https://api.github.com/repos/aaif-goose/goose/releases/latest")
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
                "source": "github:aaif-goose/goose", "package": "", "model": "qwen3.6",
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
                "source": "github:aaif-goose/goose", "package": "", "model": "qwen3.6",
                "diagnostic": {"category": "http", "httpStatus": 403},
            }]}))
            _, loaded = suite._load_manifest(path, ["goose"], "macos", "aarch64", "qwen3.6")
            self.assertEqual(loaded[0].diagnostic["httpStatus"], 403)
            path.write_text(json.dumps({"harnesses": [], "unresolved": [{
                "harness": "goose", "system": "macos", "architecture": "aarch64",
                "source": "github:aaif-goose/goose", "diagnostic": {"category": "http", "httpStatus": 403,
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
                "source": "github:aaif-goose/goose", "package": "", "model": "qwen3.6",
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
