#!/usr/bin/env python3
"""Native private-profile continuation and routing against synthetic providers."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import re
from pathlib import Path
import shutil
import socketserver
import sqlite3
import sys
import tempfile
import threading
import uuid

sys.dont_write_bytecode = True

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "actions"))
from cell import HERMES_PROFILE_DIAGNOSTICS, ensure_private_directory, private_command, protect_private
from hermes_diagnostics import (PREFIX, bounded_tail, closed_frontend_diagnostics, frontend_phase,
                                managed_profile_diagnostics, managed_profile_frames)
from processes import Gateways

KEY = "synthetic-hermes-native-key"
LAUNCH_WRAPPER = ("import os,subprocess,sys; os.environ['NAN_API_KEY']='" + KEY
                  + "'; sys.exit(subprocess.call(sys.argv[1:]))")
CHECKS = ["native-state-preserved", "same-session-resume", "bridge-route-rotation",
          "reasoning-effort", "reasoning-disabled", "reasoning-toggle",
          "gateway-cleanup", "automation-isolated"]


class Provider(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.respond({"object": "list", "data": [
            {"id": model, "object": "model", "context_length": context}
            for model, context in (("qwen3.6", 262144), ("mimo-v2.6-flash", 1000000))]})

    def do_POST(self):
        if self.headers.get("Authorization") != "Bearer " + KEY:
            self.server.unauthorized += 1
            self.send_error(401)
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.server.requests.append(body)
        model = body.get("model", "qwen3.6")
        if body.get("stream"):
            chunk = {"id": "fixture", "object": "chat.completion.chunk", "created": 1, "model": model,
                     "choices": [{"index": 0, "delta": {"role": "assistant", "content": "SYNTHETIC_REPLY"}, "finish_reason": None}]}
            finish = {**chunk, "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}
            data = ("data: " + json.dumps(chunk) + "\n\ndata: " + json.dumps(finish) + "\n\ndata: [DONE]\n\n").encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.respond({"id": "fixture", "object": "chat.completion", "model": model,
                          "choices": [{"index": 0, "message": {"role": "assistant", "content": "SYNTHETIC_REPLY"}, "finish_reason": "stop"}],
                          "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}})

    def respond(self, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def provider():
    server = ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    server.requests, server.unauthorized = [], 0
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    protect_private(path.parent)
    with path.open("w", encoding="utf-8") as stream:
        protect_private(path)
        stream.write(content)


def fingerprints(home):
    return {str(path.relative_to(home)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in home.rglob("*") if path.is_file() and not path.is_symlink()
            and path.relative_to(home).parts[0] not in {"installs", "tools"}}


def session_ids(config):
    files = list(config.glob("hermes-profiles/*/state.db"))
    if len(files) != 1:
        raise RuntimeError()
    with sqlite3.connect(files[0].resolve().as_uri() + "?mode=ro", uri=True) as database:
        return [row[0] for row in database.execute("SELECT id FROM sessions WHERE title=?", ("nanh history proof",))]


def run(args):
    root = Path(tempfile.mkdtemp(prefix="nanh-hermes-", dir=args.output.parent))
    protect_private(root)
    home, config = root / "home", root / "nan-config"
    ensure_private_directory(home)
    native = home / ".hermes"
    ensure_private_directory(native)
    source = Path(os.environ.get("HERMES_HOME") or (
        Path(os.environ["LOCALAPPDATA"]) / "hermes" if os.name == "nt" else Path.home() / ".hermes"))
    for name in ("installs", "tools"):
        if (source / name).is_dir():
            os.symlink((source / name).resolve(), native / name, target_is_directory=True)
    write(native / "config.yaml", json.dumps({"providers": {"foreign": {"api_key": "synthetic-native-secret"}},
                                              "gateway": {"multiplex_profiles": True}, "platforms": {"irc": {"enabled": True}}}))
    for name in (".env", "gateway.lock", "profiles/native/sentinel", "cron/sentinel"):
        write(native / name, "NANH_SYNTHETIC_NATIVE_STATE=preserve\n")
    write(native / ".env", "NAN_API_KEY=synthetic-native-env-key\nANTHROPIC_API_KEY=synthetic-native-env-key\n")
    write(native / "state.db", "synthetic native history must not be opened or imported\n")
    before = fingerprints(native)
    environment = dict(os.environ, HOME=str(home), HERMES_HOME=str(native), NAN_HARNESS_CONFIG_DIR=str(config),
                       NAN_NO_COMPATIBILITY_CHECK="1", NAN_NO_UPDATE_CHECK="1",
                       INVOCATION_ID="synthetic-ancestor-service", HERMES_DESKTOP_MANAGED="1",
                       HERMES_LAUNCHD_LABEL="ai.hermes.synthetic-ancestor", XPC_SERVICE_NAME="ai.hermes.synthetic-ancestor")
    environment.pop("NAN_API_KEY", None)
    environment.pop("NAN_MEDIA_API_KEY", None)
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    if os.name == "nt":
        environment.update(USERPROFILE=str(home), LOCALAPPDATA=str(home / "AppData/Local"), APPDATA=str(home / "AppData/Roaming"))
        Path(environment["LOCALAPPDATA"]).mkdir(parents=True)
        Path(environment["APPDATA"]).mkdir(parents=True)
    observer = Gateways(config)
    servers = [provider(), provider()]
    automation = []

    class NoBot(socketserver.BaseRequestHandler):
        def handle(self):
            automation.append(True)

    bot = socketserver.ThreadingTCPServer(("127.0.0.1", 0), NoBot)
    threading.Thread(target=bot.serve_forever, daemon=True).start()
    environment.update(IRC_SERVER="127.0.0.1", IRC_PORT=str(bot.server_address[1]), IRC_CHANNEL="#synthetic")
    launches = 0
    phase = "history-create"
    checks = []
    diagnostics = []
    frames = []

    def launch(server, prompt, *, reasoning=None, model="qwen3.6", continuation=False):
        nonlocal launches
        arguments = [str(args.nanh), "hermes", "--executable", str(args.hermes), "--model", model,
                     "--provider-base-url", f"http://127.0.0.1:{server.server_port}/v1", "--"]
        if reasoning:
            arguments += ["--reasoning", reasoning]
        arguments += ["chat", "--query", prompt, "--format", "stream-json", "--quiet", "--yolo", "--source", "cli"]
        if continuation:
            arguments += ["--continue", "nanh", "history", "proof", "--create-if-missing"]
        # The runner strips real credentials. Only this child supplies the fixture
        # credential, and its provider base URL is the synthetic loopback server.
        # Windows exec starts a different PID and exits the wrapper. Keep this
        # foreground process alive so its Job is not closed around a live nanh.
        launches += 1
        trace_identity = uuid.uuid4().hex
        environment["NAN_HARNESS_HERMES_DIAGNOSTICS"] = trace_identity
        def closed_diagnostic(log, status):
            text = bounded_tail(log)
            allowed = set(HERMES_PROFILE_DIAGNOSTICS)
            diagnostics.extend(sorted(set(re.findall(r"NH-[A-Z0-9-]+", text)) & allowed))
            diagnostics.extend(closed_frontend_diagnostics(text))
            if status:
                diagnostics.append(frontend_phase(text))
        status = private_command([sys.executable, "-c", LAUNCH_WRAPPER, *arguments], root, timeout=150,
                                 environment=environment, allow_failure=True, diagnostic_callback=closed_diagnostic)
        if status != 0:
            diagnostics.extend(managed_profile_diagnostics(config))
            frames.extend(managed_profile_frames(config, trace_identity))
            diagnostics.append(PREFIX + ("PROVIDER-OBSERVED" if server.requests else "PROVIDER-ABSENT"))
            if status in {1, 2, 125}:
                diagnostics.append("NH-HERMES-FRONTEND-EXIT-" + str(status))
            raise RuntimeError()

    try:
        launch(servers[0], "NANH_FIRST_TURN", continuation=True)
        phase = "history-created-state"
        first = session_ids(config)
        if len(first) != 1:
            raise RuntimeError()
        count = len(servers[0].requests)
        phase = "history-resume"
        launch(servers[1], "NANH_SECOND_TURN", continuation=True)
        phase = "history-resumed-state"
        if session_ids(config) != first or not any(
                "NANH_FIRST_TURN" in json.dumps(body) and "NANH_SECOND_TURN" in json.dumps(body)
                for body in servers[1].requests):
            raise RuntimeError()
        if not count or len(servers[0].requests) != count or not servers[1].requests:
            raise RuntimeError()
        checks += ["same-session-resume", "bridge-route-rotation"]
        for name, model, level, predicate in [
            ("reasoning-effort", "qwen3.6", "high", lambda body: body.get("reasoning_effort") == "high"),
            ("reasoning-disabled", "qwen3.6", "none", lambda body: body.get("reasoning_effort") == "none"),
            ("reasoning-toggle", "mimo-v2.6-flash", "none", lambda body: (body.get("chat_template_kwargs") or {}).get("enable_thinking") is False),
        ]:
            phase = name
            start = len(servers[1].requests)
            launch(servers[1], "NANH_REASONING_FIXTURE", model=model, reasoning=level)
            if not any(predicate(body) for body in servers[1].requests[start:]):
                raise RuntimeError()
            checks.append(name)
        phase = "native-state"
        if (fingerprints(native) != before or any(server.unauthorized for server in servers)
                or len(servers[0].requests) != count):
            raise RuntimeError()
        checks.append("native-state-preserved")
        if automation:
            raise RuntimeError()
        checks.append("automation-isolated")
    except Exception:
        pass
    finally:
        gone = observer.finish(launches)
        for server in [*servers, bot]:
            server.shutdown()
            server.server_close()
        if gone:
            checks.append("gateway-cleanup")
            shutil.rmtree(root)
    success = set(checks) == set(CHECKS)
    result = {"schemaVersion": 1, "status": "passed" if success else "failed", "checks": CHECKS if success else [],
              **({} if success else {"phase": phase, "cleanup": "proven" if gone else "unproven",
                                    "diagnostics": list(dict.fromkeys(diagnostics)), "frames": frames})}
    write(args.output, json.dumps(result) + "\n")
    return 0 if success else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--nanh", type=Path, required=True)
    parser.add_argument("--hermes", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    raise SystemExit(run(parser.parse_args()))
