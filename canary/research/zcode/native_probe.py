"""Isolated feasibility probe; not a compatibility-manifest or daily gate entry."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Scenario:
    def __init__(self):
        self.requests = []
        self.steps = []
        self.completed = set()
        self.child_seen = False
        self.key = "synthetic-only"
        self.failures = []

    def respond(self, body):
        self.requests.append(body)
        last_user = next(
            (m.get("content", "") for m in reversed(body["messages"]) if m["role"] == "user"),
            "",
        )
        child = "NATIVE_CHILD_PROBE" in str(last_user)
        self.child_seen |= child
        for message in body["messages"]:
            call_id = message.get("tool_call_id", "")
            if message["role"] == "tool" and call_id.startswith("probe-"):
                self.completed.add(call_id)
                content = str(message.get("content", ""))
                if "<tool_use_error>" in content:
                    self.failures.append(call_id)
        index = len(self.completed)
        if body.get("tools") and not child and index < len(self.steps):
            name, arguments = self.steps[index]
            delta = {"tool_calls": [{
                "index": 0,
                "id": f"probe-{index}",
                "type": "function",
                "function": {"name": name, "arguments": json.dumps(arguments)},
            }]}
            finish = "tool_calls"
        else:
            delta = {"content": "NATIVE_CHILD_OK" if child else "NATIVE_PROBE_OK"}
            finish = "stop"
        return {
            "id": "synthetic",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": body["model"],
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        }


def make_handler(scenario):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            self.send_response(404)
            self.end_headers()

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers.get("content-length", "0"))) or b"{}")
            if not self.path.endswith("/chat/completions"):
                self.send_response(404)
                self.end_headers()
                return
            if self.headers.get("authorization") != "Bearer " + scenario.key:
                scenario.failures.append("authentication")
                self.send_response(401)
                self.end_headers()
                return
            chunk = scenario.respond(body)
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.end_headers()
            self.wfile.write(("data: " + json.dumps(chunk) + "\n\ndata: [DONE]\n\n").encode())

    return Handler


def provider_config(model, key, base_url):
    return {"schemaVersion": 1, "config": {
        "providerConfigRules": {"providerRules": [{
            "providerId": "nan", "providerName": "NaN", "enabled": True,
            "config": {
                "group": "standard-personal",
                "access": {"type": "api-key", "apiKey": key},
                "api": {"type": "openai-chat-completions", "baseUrl": base_url},
                "personalModelIds": [model],
            },
        }]},
        "modelConfigRules": {
            "providerModelRules": [{"providerId": "nan", "modelId": model, "config": {}}],
            "manualProviderModelRules": [],
        },
        "defaultModelSelection": {"providerId": "nan", "modelId": model},
    }}


def run_prompt(node, cli, workspace, env):
    result = subprocess.run(
        [node, str(cli), "--prompt", "synthetic native probe", "--locale", "en-US", "--no-color"],
        cwd=workspace, env=env, capture_output=True, text=True, timeout=60,
    )
    # Child logs stay private; a failure reports only the exit status.
    assert result.returncode == 0, f"Native probe exited with status {result.returncode}"
    assert "NATIVE_PROBE_OK" in result.stdout, "Final synthetic marker missing"


def exercise_native(source, node, scenario, port):
    cli = source / "apps/zcode-cli/packages/cli/dist/zcode.cjs"
    with tempfile.TemporaryDirectory(prefix="nanh-zcode-native-") as temporary:
        home = Path(temporary)
        workspace = home / "workspace"
        workspace.mkdir()
        fixture = workspace / "fixture.txt"
        fixture.write_text("synthetic fixture\n")
        subprocess.run(["git", "init", "-q", str(workspace)], check=True)
        output = workspace / "output.txt"
        scenario.steps = [
            ("Read", {"file_path": str(fixture)}),
            ("Write", {"file_path": str(output), "content": "synthetic initial\n"}),
            ("Read", {"file_path": str(output)}),
            ("Edit", {"file_path": str(output), "old_string": "synthetic initial", "new_string": "synthetic final"}),
            ("Bash", {"command": 'test "$(cat output.txt)" = "synthetic final"'}),
            ("Agent", {"description": "Synthetic child probe", "prompt": "NATIVE_CHILD_PROBE: reply with the supplied synthetic marker."}),
        ]
        origin = f"http://127.0.0.1:{port}"
        personal = home / "provider.json"
        personal.write_text(json.dumps(provider_config("synthetic-model", scenario.key, origin + "/v1")))
        personal.chmod(0o600)
        builtin = json.loads((source / "config/provider/zcode-builtin.json").read_text())
        builtin["config"]["providerConfigRules"] = {"templateRules": [], "providerRules": []}
        restricted_builtin = home / "builtin.json"
        restricted_builtin.write_text(json.dumps(builtin))
        env = {
            "HOME": temporary, "PATH": os.environ["PATH"], "TMPDIR": temporary,
            "USERPROFILE": temporary, "TEMP": temporary, "TMP": temporary,
            "APPDATA": temporary, "LOCALAPPDATA": temporary,
            "ZCODE_DATA_BASE_DIR": temporary,
            "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE": str(personal),
            "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE": str(restricted_builtin),
            "ZCODE_ENDPOINT_ORIGIN": origin,
            "ZCODE_MODEL_TELEMETRY_ENABLED": "0", "ZCODE_TELEMETRY_REPORT_ENDPOINT": "",
            "HTTP_PROXY": "http://127.0.0.1:9", "HTTPS_PROXY": "http://127.0.0.1:9",
            "NO_PROXY": "127.0.0.1,localhost",
        }
        for name in ("SYSTEMROOT", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT", "SystemDrive"):
            if name in os.environ:
                env[name] = os.environ[name]
        run_prompt(node, cli, workspace, env)
        assert len(scenario.completed) == len(scenario.steps), "Tool continuation incomplete"
        assert scenario.child_seen, "Child agent did not reach the synthetic provider"
        assert not scenario.failures, "A tool or authentication check failed"
        assert output.read_text() == "synthetic final\n", "Expected file effect missing"
        assert scenario.requests and all(r["model"] == "synthetic-model" for r in scenario.requests)
        print(f"PASS: native tools, child agent and selected model ({len(scenario.requests)} requests)")

        scenario.requests.clear()
        scenario.steps.clear()
        scenario.completed.clear()
        scenario.key = "synthetic-rotated"
        personal.write_text(json.dumps(provider_config("synthetic-second", scenario.key, origin + "/v1")))
        run_prompt(node, cli, workspace, env)
        assert not scenario.failures
        assert scenario.requests and all(r["model"] == "synthetic-second" for r in scenario.requests)
        print("PASS: fresh launch observes replacement model catalog and rotated credential")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--node", required=True)
    args = parser.parse_args()
    scenario = Scenario()
    server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(scenario))
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        exercise_native(args.source.resolve(), args.node, scenario, server.server_port)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


if __name__ == "__main__":
    main()
