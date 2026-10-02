"""Exercise the actual nanh installer, managed launcher and native configuration."""

import argparse
from contextlib import nullcontext
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from http.server import ThreadingHTTPServer

from native_probe import Scenario, make_handler
from terminal import Terminal


def supervised_windows_probe():
    # The coordinator outlives individual launches. Own the whole probe tree so
    # its background processes release the temporary workspace before cleanup.
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "actions"))
    from cell import WindowsJob

    with tempfile.TemporaryDirectory(prefix="nanh-zcode-integration-") as home:
        child = subprocess.Popen([sys.executable, __file__, *sys.argv[1:],
                                  "--worker-home", home], creationflags=0x00000004)
        job = None
        try:
            job = WindowsJob(child.pid)
            job.resume(child.pid)
            result = child.wait(timeout=1800)
        finally:
            if job:
                job.close()
            else:
                child.kill()
            child.wait(timeout=10)
        if result != 0:
            raise SystemExit(result)


def command(arguments, workspace, environment, marker=None):
    result = subprocess.run(arguments, cwd=workspace, env=environment,
                            capture_output=True, text=True, timeout=90)
    if result.returncode != 0:
        with tempfile.NamedTemporaryFile(prefix="nanh-zcode-synthetic-command-failure-", delete=False) as failure:
            failure.write((result.stdout + result.stderr).encode())
        raise AssertionError(f"Integration command exited {result.returncode}")
    if marker:
        assert marker in result.stdout, "Synthetic completion marker missing"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--worker-home", help=argparse.SUPPRESS)
    args = parser.parse_args()
    if os.name == "nt" and not args.worker_home:
        supervised_windows_probe()
        return
    binary = args.binary.resolve()
    scenario = Scenario()
    searches = []
    scenario.models = ["synthetic-model", "synthetic-second"]
    base = make_handler(scenario)

    class Handler(base):
        def do_GET(self):
            if self.path.startswith("/search?"):
                assert not self.headers.get("authorization"), "Search received a provider key"
                searches.append(self.path)
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"results": [{"title": "SYNTHETIC_SEARCH_RESULT", "url": "https://synthetic.test/result", "content": "synthetic snippet"}]}).encode())
            elif self.path.endswith("/models"):
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps({"data": [{"id": model} for model in scenario.models]}).encode())
            else:
                super().do_GET()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        temporary_home = (nullcontext(args.worker_home) if args.worker_home else
                          tempfile.TemporaryDirectory(prefix="nanh-zcode-integration-"))
        with temporary_home as temporary:
            home = Path(temporary)
            workspace = home / "workspace"
            workspace.mkdir()
            subprocess.run(["git", "init", "-q", str(workspace)], check=True)
            state = home / "nan-state"
            state.mkdir(mode=0o700)
            (state / "nan-api-key").write_text(scenario.key)
            (state / "nan-api-key").chmod(0o600)
            (state / "credential.json").write_text('{"schemaVersion":1,"backend":"private-file"}')
            (state / "credential.json").chmod(0o600)
            user_config = home / ".zcode/cli/config.json"
            user_config.parent.mkdir(parents=True)
            user_config.write_text('{"theme":"user-owned","features":{"mcp":false}}')
            original = user_config.read_bytes()
            origin = f"http://127.0.0.1:{server.server_port}"
            environment = {name: value for name, value in os.environ.items()
                           if name in ("PATH", "SYSTEMROOT", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "SystemDrive")}
            helper = binary.parent / ("nan-harness.exe" if os.name == "nt" else "nan-harness")
            assert helper.is_file(), "Build both nanh and nan-harness before running the integration probe"
            # Native MCP commands resolve nan-harness through PATH. Test this checkout,
            # even when another nan-harness version is installed on the host.
            environment["PATH"] = str(binary.parent) + os.pathsep + environment.get("PATH", "")
            environment.update(HOME=temporary, USERPROFILE=temporary, APPDATA=temporary,
                               LOCALAPPDATA=temporary, TMPDIR=temporary, TMP=temporary, TEMP=temporary,
                               NAN_HARNESS_CONFIG_DIR=str(state), NAN_HARNESS_CREDENTIAL_BACKEND="file",
                               NAN_NO_COMPATIBILITY_CHECK="1", NAN_BASE_URL=origin + "/v1",
                               NAN_API_KEY=scenario.key, NAN_HARNESS_TELEMETRY="off",
                               ZCODE_ENDPOINT_ORIGIN=origin, ZCODE_MODEL_TELEMETRY_ENABLED="0",
                               ZCODE_TELEMETRY_REPORT_ENDPOINT="", TERM="xterm-256color")
            native = ["--locale", "en-US", "--no-color", "--prompt", "synthetic integration probe"]
            prefix = [str(binary), "zcode", "--model", "synthetic-model"]
            if args.source:
                # Existing-source mode tests routing separately from the download/build contract.
                executable = home / ("zcode.cmd" if os.name == "nt" else "zcode")
                cli = args.source.resolve() / "apps/zcode-cli/packages/cli/dist/zcode.cjs"
                executable.write_text(f'@echo off\nnode "{cli}" %*\n' if os.name == "nt"
                                      else f'#!/bin/sh\nexec node "{cli}" "$@"\n')
                executable.chmod(0o755)
                prefix += ["--executable", str(executable)]
            else:
                terminal = Terminal([*prefix, "--", *native], workspace, environment)
                try:
                    terminal.write(b"y\r")
                    deadline = time.monotonic() + 1200
                    output = bytearray()
                    while terminal.alive() and time.monotonic() < deadline:
                        try:
                            output.extend(terminal.read())
                        except OSError:
                            break
                        assert len(output) < 8 * 1024 * 1024, "Installer output exceeded probe bound"
                    assert not terminal.alive(), "Source installation exceeded its time bound"
                    # An exited child can still leave its final output buffered in the PTY.
                    while True:
                        try:
                            remaining = terminal.read()
                        except (OSError, EOFError):
                            break
                        if not remaining:
                            break
                        output.extend(remaining)
                        assert len(output) < 8 * 1024 * 1024, "Installer output exceeded probe bound"
                    codes = sorted(set(re.findall(r"NH-[A-Z]+-[0-9]+|ERR_PNPM_[A-Z_]+", output.decode("utf8", "replace"))))
                    if b"NATIVE_PROBE_OK" not in output:
                        # The isolated provider and prompts are synthetic. Keep the failed
                        # terminal transcript in an owner-only temporary file for local diagnosis.
                        with tempfile.NamedTemporaryFile(prefix="nanh-zcode-synthetic-failure-", delete=False) as failure:
                            failure.write(output)
                        raise AssertionError(f"Installed CLI did not complete its first launch; diagnostic codes: {codes}")
                finally:
                    terminal.close()
                executable = home / ".local/bin" / ("zcode.cmd" if os.name == "nt" else "zcode")
                assert executable.is_file(), "Installer did not publish the command"
                print("PASS: automatic official-source installation and first managed launch")

            command([*prefix, "--dry-run"], workspace, {k: v for k, v in environment.items() if k != "NAN_API_KEY"})
            scenario.requests.clear()
            output = workspace / "output.txt"
            scenario.steps = [("Write", {"file_path": str(output), "content": "managed marker\n"}),
                              ("Read", {"file_path": str(output)}),
                              ("Edit", {"file_path": str(output), "old_string": "managed", "new_string": "verified"}),
                              ("Agent", {"description": "Synthetic child", "prompt": "NATIVE_CHILD_PROBE"})]
            command([*prefix, "--", *native], workspace, environment, "NATIVE_PROBE_OK")
            assert output.read_text() == "verified marker\n"
            assert len(scenario.completed) == len(scenario.steps) and scenario.child_seen
            assert not scenario.failures
            assert all(request["model"] == "synthetic-model" for request in scenario.requests)
            assert user_config.read_bytes() == original
            assert not list(home.glob("nan-harness-*")), "Private launch artifacts remained"
            print("PASS: managed tools, child model inheritance, preserved user settings and cleanup")

            (state / "search.json").write_text(json.dumps({"schemaVersion": 1, "mode": "local", "baseUrl": origin + "/"}))
            (state / "search.json").chmod(0o600)
            scenario.steps.clear()
            scenario.completed.clear()
            scenario.requests.clear()
            respond = scenario.respond

            def request_search(body):
                if not scenario.steps and body.get("tools"):
                    names = [tool["function"]["name"] for tool in body["tools"] if tool["function"]["name"].endswith("web_search")]
                    if len(names) != 1:
                        scenario.failures.append("Managed search tool missing or duplicated")
                        return respond(body)
                    scenario.steps.append((names[0], {"query": "synthetic", "max_results": 1}))
                return respond(body)

            scenario.respond = request_search
            result = command([*prefix, "--", *native], workspace, environment, "NATIVE_PROBE_OK")
            if len(searches) != 1:
                with tempfile.NamedTemporaryFile(prefix="nanh-zcode-synthetic-search-failure-", delete=False) as failure:
                    failure.write((result.stdout + result.stderr).encode())
            assert len(searches) == 1, f"Managed search requests: {len(searches)}; failures: {scenario.failures}"
            assert "SYNTHETIC_SEARCH_RESULT" in json.dumps(scenario.requests[-1]["messages"])
            assert user_config.read_bytes() == original
            scenario.respond = respond
            print("PASS: private managed search MCP inventory, execution and result continuation")

            scenario.steps.clear()
            scenario.completed.clear()
            environment.pop("NAN_API_KEY")
            command([str(binary), "config", "zai", "--yes", "--no-search"], workspace, environment)
            provider = home / ".zcode/v2/provider_config.json"
            assert provider.is_file()
            command([str(binary), "config", "zcode", "--status"], workspace, environment)
            receipt = (state / "configurations.json").read_text()
            assert scenario.key not in receipt
            command([str(executable), *native], workspace, environment, "NATIVE_PROBE_OK")
            scenario.models = ["replacement"]
            scenario.key = "synthetic-rotated"
            (state / "nan-api-key").write_text(scenario.key)
            scenario.requests.clear()
            command([str(binary), "config", "zcode", "--refresh"], workspace, environment)
            command([str(executable), *native], workspace, environment, "NATIVE_PROBE_OK")
            assert scenario.requests and all(request["model"] == "replacement" for request in scenario.requests)
            command([str(binary), "config", "zcode", "--remove"], workspace, environment)
            assert not provider.exists()
            assert json.loads(user_config.read_bytes()) == json.loads(original)
            print("PASS: native configure/status/refresh/rotation/remove and standalone zcode")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    main()
