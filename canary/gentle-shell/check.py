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
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for delta, finish in [({"role": "assistant", "content": "GENTLE_ROUTING_OK"}, None), ({}, "stop")]:
            event = {
                "id": "chatcmpl-fixture", "object": "chat.completion.chunk", "created": 1,
                "model": "qwen3.6", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13},
            }
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.write(b"data: [DONE]\n\n")


def write_json(path, value):
    path.write_text(json.dumps(value))
    path.chmod(0o600)


def environment(home):
    env = dict(os.environ)
    for key in ["NAN_API_KEY", "NAN_BASE_URL", "GENTLE_SHELL_HOME", "PI_CODING_AGENT_DIR", "GENTLE_SHELL_USER_PI_HOME"]:
        env.pop(key, None)
    env.update({
        "HOME": str(home), "USERPROFILE": str(home),
        "NAN_HARNESS_CONFIG_DIR": str(home / "nanh"), "NAN_HARNESS_CREDENTIAL_BACKEND": "file",
        "PI_OFFLINE": "1", "GENTLE_SHELL_NO_AUTO_SETUP": "1", "GENTLE_AI_TELEMETRY": "0", "ENGRAM_STARTUP_POLICY": "deny",
        "NAN_NO_UPDATE_CHECK": "1", "NAN_NO_COMPATIBILITY_CHECK": "1",
    })
    return env


def launch(args, home, agent, flags):
    return subprocess.run([
        str(args.nanh), "gentle-shell", "--executable", str(args.gentle_shell), *flags,
        "--", "--home", str(agent), "--no-session", "-p", "Reply GENTLE_ROUTING_OK without tools.",
    ], cwd=home, env=environment(home), capture_output=True, text=True, timeout=90)


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
        assert result.returncode == 0, result.stderr
        assert "GENTLE_ROUTING_OK" in result.stdout, result.stdout
        assert ("13 tokens" in result.stderr) == (not flags), result.stderr
        assert all((agent / name).read_bytes() == value for name, value in original.items())
        assert "newer than" not in result.stderr, result.stderr
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
    assert result.returncode == 0, result.stderr
    assert "GENTLE_ROUTING_OK" in result.stdout
    assert len(server.requests) == 3
    assert "synthetic-native-key" not in (agent / "auth.json").read_text()
    print("PASS direct fallback to nanh credentials without saving them in Gentle Shell")

    write_json(agent / "auth.json", {"nan": {"type": "api_key", "key": ""}})
    result = launch(args, home, agent, ["--no-chat-gateway", "--provider-base-url", url])
    assert result.returncode != 0
    assert len(server.requests) == 3
    print("PASS invalid native credentials do not silently switch accounts")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--nanh", type=Path, required=True)
    parser.add_argument("--gentle-shell", type=Path, required=True)
    args = parser.parse_args()
    args.nanh = args.nanh.absolute()
    args.gentle_shell = args.gentle_shell.absolute()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    server.requests = []
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="nanh-gentle-contract-") as directory:
            check_routing(args, Path(directory), server)
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
