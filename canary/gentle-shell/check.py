#!/usr/bin/env python3
"""Isolated Gentle Shell/Pi routing contract against a synthetic provider.

Install gentle-pi 4.1.0 and Pi 1.1.0 in a disposable npm prefix first.
This probe never reads real credentials or contacts NaN.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import sys
import shutil
from live_provider import LiveProvider
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
from cell import private_command, protect_private
from scenarios import child_prompt, gateway_requests, reader_fixture, verify_child


class Provider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def authorized(self):
        valid = self.headers.get("Authorization") == "Bearer synthetic-native-key"
        if not valid:
            self.send_response(401)
            self.end_headers()
        return valid

    def do_GET(self):
        if self.path.startswith("/search?"):
            self.server.searches += 1
            data = {"results": [{"title": "MANAGED_SEARCH_OK", "url": "https://example.test/fixture",
                                 "content": "MANAGED_SEARCH_OK"}]}
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(data).encode())
            return
        if not self.authorized():
            return
        data = {"object": "list", "data": [{"id": "qwen3.6", "object": "model", "owned_by": "nan"}]}
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def do_POST(self):
        if not self.authorized():
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests.append((self.path, body.get("model")))
        try:
            delta, finish = self.reply(body)
        except (AssertionError, KeyError, TypeError):
            self.server.failed = True
            delta, finish = {"role": "assistant", "content": "FIXTURE_FAILED"}, "stop"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for delta, finish in [(delta, None), ({}, finish)]:
            event = {
                "id": "chatcmpl-fixture", "object": "chat.completion.chunk", "created": 1,
                "model": "qwen3.6", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13},
            }
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.write(b"data: [DONE]\n\n")


    def reply(self, body):
        if self.server.scenario == "routing":
            return {"role": "assistant", "content": "GENTLE_ROUTING_OK"}, "stop"
        if self.server.scenario in ("native-tools", "forced-search"):
            results = [message for message in body["messages"] if message.get("role") == "tool"]
            if results:
                text = json.dumps(results)
                expected = "MANAGED_SEARCH_OK" if self.server.scenario == "forced-search" else "NATIVE_SEARCH_OK"
                assert expected in text and "NATIVE_MEMORY_OK" in text, "native-tool-result"
                self.server.proven.add(self.server.scenario)
                return {"role": "assistant", "content": "NATIVE_TOOLS_OK"}, "stop"
            calls = [{"index": index, "id": "fixture-" + name, "type": "function",
                      "function": {"name": name, "arguments": '{"query":"fixture"}'}}
                     for index, name in enumerate(("web_search", "mem_search"))]
            return {"role": "assistant", "tool_calls": calls}, "tool_calls"
        names = {tool["function"]["name"] for tool in body.get("tools", [])}
        child = "subagent_run" not in names
        tool = "read" if child else "subagent_run"
        results = [message for message in body["messages"] if message.get("role") == "tool"]
        if results:
            assert self.server.marker in json.dumps(results), "fixture-result"
            self.server.proven.add("child" if child else "parent")
            return {"role": "assistant", "content": self.server.marker}, "stop"
        arguments = {"path": "sentinel.txt"} if child else {
            "agent": "qualification-reader", "mode": "task",
            "task": "Read sentinel.txt with read and return its exact contents"}
        return {"role": "assistant", "tool_calls": [{"index": 0, "id": "fixture-call",
                "type": "function", "function": {"name": tool, "arguments": json.dumps(arguments)}}]}, "tool_calls"


def write_json(path, value):
    path.write_text(json.dumps(value))
    path.chmod(0o600)
    protect_private(path)


def environment(home):
    env = dict(os.environ)
    for key in ["NAN_API_KEY", "NAN_BASE_URL", "NAN_PROVIDER_BASE_URL", "GENTLE_SHELL_CHILD_PACKAGE_INJECTION", "GENTLE_SHELL_HOME", "PI_CODING_AGENT_DIR", "GENTLE_SHELL_USER_PI_HOME"]:
        env.pop(key, None)
    for key, suffix in (("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                        ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state"),
                        ("APPDATA", "appdata"), ("LOCALAPPDATA", "localappdata"),
                        ("TMPDIR", "tmp"), ("TMP", "tmp"), ("TEMP", "tmp")):
        directory = home / suffix
        directory.mkdir(mode=0o700, exist_ok=True)
        protect_private(directory)
        env[key] = str(directory)
    env.update({
        "HOME": str(home), "USERPROFILE": str(home),
        "NAN_HARNESS_CONFIG_DIR": str(home / "nanh"), "NAN_HARNESS_CREDENTIAL_BACKEND": "file",
        "PI_OFFLINE": "1", "GENTLE_SHELL_NO_AUTO_SETUP": "1", "GENTLE_AI_TELEMETRY": "0", "ENGRAM_STARTUP_POLICY": "deny",
        "NAN_NO_UPDATE_CHECK": "1", "NAN_NO_COMPATIBILITY_CHECK": "1",
    })
    return env


def launch(args, home, agent, flags, prompt="Reply GENTLE_ROUTING_OK without tools.", live=False):
    env = environment(home)
    if live:
        env["NAN_API_KEY"] = os.environ["NAN_API_KEY"]
    command = [
        str(args.nanh), "gentle-shell", "--executable", str(args.gentle_shell), *flags,
        "--", "--home", str(agent), "--no-session", "--mode", "json", "-p", prompt,
    ]
    output = home / "process-output-private.txt"
    captured = {}
    def capture(log, _status):
        log.seek(0)
        captured["stderr"] = log.read(8_000_001).decode("utf-8", errors="replace")
        assert len(captured["stderr"]) <= 8_000_000, "stderr-bound"
    try:
        status = private_command(command, home, timeout=180, output=output, live=live,
                                 allow_failure=True, environment=env, diagnostic_callback=capture)
        assert output.stat().st_size <= 8_000_000, "stdout-bound"
        return subprocess.CompletedProcess(command, status, output.read_text(encoding="utf-8"), captured["stderr"])
    finally:
        output.unlink(missing_ok=True)


def check_routing(args, home, server):
    agent = home / "agent"
    agent.mkdir()
    url = f"http://127.0.0.1:{server.server_port}/v1"
    write_json(agent / "auth.json", {"nan": {"type": "api_key", "key": "synthetic-native-key"}})
    write_json(agent / "models.json", {"providers": {"nan": {"baseUrl": url}}})
    write_json(agent / "settings.json", {"defaultProvider": "nan", "defaultModel": "qwen3.6"})
    original = {name: (agent / name).read_bytes() for name in ["auth.json", "models.json"]}
    for flags in [["--no-chat-gateway"], []]:
        result = launch(args, home, agent, flags)
        assert result.returncode == 0, "launcher-exit"
        assert "GENTLE_ROUTING_OK" in result.stdout, "fixture-response"
        assert ("13 tokens" in result.stderr) == (not flags), "gateway-accounting"
        assert all((agent / name).read_bytes() == value for name, value in original.items())
        assert "newer than" not in result.stderr, "gateway-accounting"
    assert len(server.requests) == 2
    assert all(path == "/v1/chat/completions" and model == "qwen3.6" for path, model in server.requests)
    print("PASS native credentials, native direct routing, gateway routing and accounting, immutable native provider files")

    # No native provider credential: use nanh's private synthetic credential.
    (agent / "auth.json").unlink()
    write_json(agent / "models.json", {})
    state = home / "nanh"
    state.mkdir(exist_ok=True)
    state.chmod(0o700)
    (state / "nan-api-key").write_text("synthetic-native-key")
    (state / "nan-api-key").chmod(0o600)
    write_json(state / "credential.json", {"schemaVersion": 1, "backend": "private-file"})
    result = launch(args, home, agent, ["--no-chat-gateway", "--provider-base-url", url])
    assert result.returncode == 0, "launcher-exit"
    assert "GENTLE_ROUTING_OK" in result.stdout
    assert len(server.requests) == 3
    assert "synthetic-native-key" not in (agent / "auth.json").read_text()
    print("PASS direct fallback to nanh credentials without saving them in Gentle Shell")

    write_json(agent / "auth.json", {"nan": {"type": "api_key", "key": ""}})
    result = launch(args, home, agent, ["--no-chat-gateway", "--provider-base-url", url])
    assert result.returncode != 0
    assert len(server.requests) == 3
    print("PASS invalid native credentials do not silently switch accounts")
    write_json(agent / "auth.json", {"nan": {"type": "api_key", "key": "synthetic-native-key"}})
    write_json(agent / "models.json", {"providers": {"nan": {"baseUrl": url}}})
    server.scenario = "child"
    server.marker = reader_fixture(home, "qwen3.6")
    server.proven = set()
    result = launch(args, home, agent, [], child_prompt())
    assert result.returncode == 0, "child-launch"
    verify_child(agent, result.stdout, server.marker, "qwen3.6")
    assert server.proven == {"parent", "child"}, "child-gateway-routing"
    assert len(server.requests) == 7, "parent-child-request-count"
    print("PASS parent and real subagent share the authenticated gateway and read a fresh fixture")
    extensions = agent / "extensions"
    extensions.mkdir(exist_ok=True)
    shutil.copyfile(Path(__file__).with_name("native-tools.mjs"), extensions / "qualification.js")
    memory = agent / "memory-fixture.json"
    write_json(memory, {"NATIVE_SEARCH_OK": "NATIVE_SEARCH_OK", "NATIVE_MEMORY_OK": "NATIVE_MEMORY_OK"})
    original_memory = memory.read_bytes()
    for flags in [[], ["--no-search"], ["--force-search"]]:
        forced = "--force-search" in flags
        server.scenario = "forced-search" if forced else "native-tools"
        if forced:
            write_json(state / "search.json", {"schemaVersion": 1, "mode": "local",
                                               "baseUrl": f"http://127.0.0.1:{server.server_port}/"})
        result = launch(args, home, agent, flags)
        assert result.returncode == 0 and "NATIVE_TOOLS_OK" in result.stdout, "native-tools"
        assert memory.read_bytes() == original_memory, "memory-preservation"
        assert server.searches == int(forced), "search-precedence"
    assert {"native-tools", "forced-search"}.issubset(server.proven), "tool-evidence"
    print("PASS native search and memory fixtures survive, forced search reaches configured backend")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nanh", type=Path, required=True)
    parser.add_argument("--gentle-shell", type=Path, required=True)
    parser.add_argument("--live", action="store_true")
    parser.add_argument("--model", default="qwen3.6")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    args.nanh = args.nanh.absolute()
    args.gentle_shell = args.gentle_shell.absolute()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), LiveProvider if args.live else Provider)
    server.requests = []
    server.scenario = "routing"
    server.searches = 0
    server.failed = False
    if args.live:
        server.key = os.environ.get("NAN_API_KEY", "")
        server.model = args.model
        server.parent_seen = server.child_seen = False
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="nanh-gentle-contract-") as directory:
            home = Path(directory)
            protect_private(home)
            if args.live:
                assert os.environ.get("NAN_API_KEY"), "missing-live-key"
                agent = home / "agent"
                agent.mkdir()
                marker = reader_fixture(home, args.model)
                result = launch(args, home, agent, ["--model", args.model, "--provider-base-url",
                                f"http://127.0.0.1:{server.server_port}/v1"], child_prompt(), live=True)
                assert result.returncode == 0, "live-launch"
                verify_child(agent, result.stdout, marker, args.model)
                assert gateway_requests(result.stderr) >= 4, "live-gateway-accounting"
                assert server.parent_seen and server.child_seen and not server.failed, "live-child-routing"
                checks = ["live-parent-tool", "live-child-read", "live-gateway-accounting"]
            else:
                check_routing(args, home, server)
                assert not server.failed, "scripted-provider"
                checks = ["native-routing", "gateway-routing", "native-preservation", "direct-fallback",
                          "invalid-native-credential", "child-gateway-routing", "native-search-memory", "forced-search"]
            if args.output:
                write_json(args.output, {"schemaVersion": 1, "status": "passed", "checks": checks})
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    try:
        main()
    except (AssertionError, OSError, ValueError, RuntimeError, subprocess.SubprocessError):
        # Never emit captured process output or provider data into hosted logs.
        print("Gentle Shell qualification failed; no private process output retained.", file=sys.stderr)
        sys.exit(1)
