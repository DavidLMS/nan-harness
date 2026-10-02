"""Verify actual ZCode session, MCP and terminal workflows with isolated state."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import time
from http.server import ThreadingHTTPServer

from native_probe import Scenario, make_handler, provider_config
from terminal import Terminal


class Runtime:
    def __init__(self, source, node, binary):
        self.source, self.node, self.binary = source, node, binary
        self.temporary = tempfile.TemporaryDirectory(prefix="nanh-zcode-runtime-")
        self.home = Path(self.temporary.name)
        self.workspace = self.home / "workspace"
        self.workspace.mkdir()
        subprocess.run(["git", "init", "-q", str(self.workspace)], check=True)
        self.scenario = Scenario()
        self.searches = 0
        base = make_handler(self.scenario)
        owner = self

        class Handler(base):
            def do_GET(self):
                if self.path.startswith("/search?"):
                    owner.searches += 1
                    assert not self.headers.get("authorization"), "Search received a provider key"
                    self.send_response(200)
                    self.send_header("content-type", "application/json")
                    self.end_headers()
                    self.wfile.write(
                        json.dumps(
                            {
                                "results": [
                                    {
                                        "title": "SYNTHETIC_SEARCH_RESULT",
                                        "url": "https://synthetic.test/result",
                                        "content": "synthetic snippet",
                                    }
                                ]
                            }
                        ).encode()
                    )
                else:
                    super().do_GET()

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        origin = f"http://127.0.0.1:{self.server.server_port}"
        self.personal = self.home / "provider.json"
        config = provider_config("synthetic-model", self.scenario.key, origin + "/v1")
        config["config"]["providerConfigRules"]["providerRules"][0]["config"][
            "personalModelIds"
        ].append("synthetic-second")
        config["config"]["modelConfigRules"]["providerModelRules"].append(
            {"providerId": "nan", "modelId": "synthetic-second", "config": {}}
        )
        self.personal.write_text(json.dumps(config))
        self.personal.chmod(0o600)
        builtin = json.loads((source / "config/provider/zcode-builtin.json").read_text())
        builtin["config"]["providerConfigRules"] = {"templateRules": [], "providerRules": []}
        (self.home / "builtin.json").write_text(json.dumps(builtin))
        self.env = {
            "HOME": str(self.home),
            "USERPROFILE": str(self.home),
            "PATH": os.environ["PATH"],
            "TMPDIR": str(self.home),
            "TMP": str(self.home),
            "TEMP": str(self.home),
            "ZCODE_DATA_BASE_DIR": str(self.home),
            "ZCODE_ENDPOINT_ORIGIN": origin,
            "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE": str(self.personal),
            "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE": str(self.home / "builtin.json"),
            "ZCODE_MODEL_TELEMETRY_ENABLED": "0",
            "ZCODE_TELEMETRY_REPORT_ENDPOINT": "",
            "HTTP_PROXY": "http://127.0.0.1:9",
            "HTTPS_PROXY": "http://127.0.0.1:9",
            "NO_PROXY": "127.0.0.1,localhost",
            "TERM": "xterm-256color",
            "COLORTERM": "truecolor",
        }
        self.env.update(APPDATA=str(self.home), LOCALAPPDATA=str(self.home))
        for name in ("SYSTEMROOT", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "SystemDrive"):
            if name in os.environ:
                self.env[name] = os.environ[name]
        self.cli = source / "apps/zcode-cli/packages/cli/dist/zcode.cjs"

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.temporary.cleanup()

    def prompt(self, text, *arguments):
        return subprocess.run(
            [
                self.node,
                str(self.cli),
                "--locale",
                "en-US",
                "--no-color",
                "--prompt",
                text,
                *arguments,
            ],
            cwd=self.workspace,
            env=self.env,
            capture_output=True,
            text=True,
            timeout=60,
        )

    def sessions(self):
        first = self.prompt("FIRST_SESSION_PROBE")
        assert first.returncode == 0
        resumed = self.prompt("CONTINUE_SESSION_PROBE", "--continue")
        assert resumed.returncode == 0
        assert "FIRST_SESSION_PROBE" in json.dumps(self.scenario.requests[-1]["messages"])
        session_ids = set(re.findall(r"sess_[A-Za-z0-9_-]+", first.stdout + first.stderr))
        for path in self.home.rglob("*"):
            session_ids.update(re.findall(r"sess_[A-Za-z0-9_-]+", path.name))
        assert session_ids, "No persisted session identifier found"
        explicit = self.prompt("EXPLICIT_RESUME_PROBE", "--resume", sorted(session_ids)[0])
        assert explicit.returncode == 0
        assert "CONTINUE_SESSION_PROBE" in json.dumps(self.scenario.requests[-1]["messages"])
        print("PASS: --continue and explicit --resume preserve the real session history")
        config = json.loads(self.personal.read_text())
        config["config"]["providerConfigRules"]["providerRules"][0]["config"][
            "personalModelIds"
        ] = ["synthetic-second"]
        config["config"]["modelConfigRules"]["providerModelRules"] = [
            {"providerId": "nan", "modelId": "synthetic-second", "config": {}}
        ]
        config["config"]["defaultModelSelection"]["modelId"] = "synthetic-second"
        self.personal.write_text(json.dumps(config))
        count = len(self.scenario.requests)
        removed = self.prompt("UNAVAILABLE_RESUME_PROBE", "--continue")
        assert (
            removed.returncode != 0 and len(self.scenario.requests) == count
        ), "Resumption silently replaced an unavailable model"
        print("PASS: resuming an unavailable model fails before contacting the provider")

    def mcp(self):
        assert self.binary is not None
        directory = self.home / "nan-config"
        directory.mkdir(mode=0o700)
        search = directory / "search.json"
        search.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "mode": "local",
                    "baseUrl": f"http://127.0.0.1:{self.server.server_port}/",
                }
            )
        )
        search.chmod(0o600)
        self.env["NAN_HARNESS_CONFIG_DIR"] = str(directory)
        user_config = self.home / ".zcode/cli/config.json"
        user_config.parent.mkdir(parents=True)
        user_config.write_text(
            json.dumps(
                {
                    "features": {"mcp": True},
                    "mcp": {
                        "servers": {
                            "nan-search": {
                                "type": "stdio",
                                "command": str(self.binary),
                                "args": ["__search-mcp"],
                                "enabled": True,
                            }
                        }
                    },
                }
            )
        )
        original = user_config.read_bytes()
        handshake = subprocess.run(
            [str(self.binary), "__search-mcp"],
            input='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}\n',
            env=self.env,
            capture_output=True,
            text=True,
            timeout=15,
        )
        assert (
            handshake.returncode == 0
            and json.loads(handshake.stdout)["result"]["serverInfo"]["name"] == "nan-search"
        )
        respond = self.scenario.respond

        def request_search(body):
            if not self.scenario.steps:
                names = [
                    t["function"]["name"]
                    for t in body.get("tools", [])
                    if t["function"]["name"].endswith("web_search")
                ]
                if not body.get("tools"):
                    return respond(body)
                if len(names) != 1:
                    self.scenario.failures.append("missing MCP tool")
                    return respond(body)
                self.scenario.steps.append((names[0], {"query": "synthetic", "max_results": 1}))
            return respond(body)

        self.scenario.respond = request_search
        result = self.prompt("NATIVE_MCP_PROBE")
        assert result.returncode == 0 and self.searches == 1
        assert "SYNTHETIC_SEARCH_RESULT" in json.dumps(self.scenario.requests[-1]["messages"])
        assert user_config.read_bytes() == original
        print(
            "PASS: actual NaN search MCP inventory, execution, result continuation and unchanged user config"
        )

    def tui(self):
        terminal = Terminal(
            [self.node, str(self.cli), "tui", "--locale", "en-US"], self.workspace, self.env
        )
        output = bytearray()

        def wait_for(condition, timeout=20):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if condition():
                    return
                if not terminal.alive():
                    raise AssertionError("TUI exited before the expected observation")
                chunk = terminal.read()
                if chunk:
                    output.extend(chunk)
                    if b"\x1b[6n" in chunk:
                        terminal.write(b"\x1b[1;1R")
            raise AssertionError("TUI observation timed out")

        try:
            wait_for(lambda: b"synthetic-model" in output)
            terminal.write(b"/model nan/synthetic-second\r")
            wait_for(
                lambda: json.loads(self.personal.read_text())["config"]["defaultModelSelection"][
                    "modelId"
                ]
                == "synthetic-second"
            )
            wait_for(lambda: b"Model switched" in output)
            time.sleep(0.5)
            terminal.write(b"\x1b")
            time.sleep(0.2)
            terminal.write(b"\x7f" * 64)
            time.sleep(0.2)
            terminal.write(b"TUI_NATIVE_PROBE\r")
            wait_for(
                lambda: self.scenario.requests
                and self.scenario.requests[-1]["model"] == "synthetic-second"
            )
            wait_for(lambda: b"NATIVE_PROBE_OK" in output)
            terminal.write(b"\x03")
            time.sleep(0.2)
            if terminal.alive():
                terminal.write(b"\x03")
            terminal.wait_exit()
            print(
                "PASS: rendered TUI, native /model switch, selected-model request, streamed answer and Ctrl-C exit"
            )
        finally:
            terminal.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--node", required=True)
    parser.add_argument("--nan-binary", type=Path)
    parser.add_argument("--case", choices=["sessions", "mcp", "tui"], required=True)
    args = parser.parse_args()
    runtime = Runtime(
        args.source.resolve(), args.node, args.nan_binary.resolve() if args.nan_binary else None
    )
    try:
        getattr(runtime, args.case)()
    finally:
        runtime.close()


if __name__ == "__main__":
    main()
