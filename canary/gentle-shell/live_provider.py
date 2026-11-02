"""Observe live routing through completed streams without retaining traffic."""
import http.server
import json
import re
import threading
import urllib.error
import urllib.request
from contextlib import contextmanager


class Observation:
    def __init__(self):
        self.condition = threading.Condition()
        self.pending = 0
        self.parent_seen = self.child_seen = False
        self.failures = set()

    @contextmanager
    def request(self):
        with self.condition:
            self.pending += 1
        try:
            yield
        finally:
            with self.condition:
                self.pending -= 1
                self.condition.notify_all()

    def wait(self, timeout=10):
        with self.condition:
            return self.condition.wait_for(lambda: self.pending == 0, timeout)


def forward_body(response, output):
    """Stop at the SSE terminator; EOF may arrive after the CLI has exited."""
    streaming = "text/event-stream" in response.headers.get("Content-Type", "")
    tail = b""
    while chunk := response.read1(65536):
        window = (tail + chunk).replace(b"\r\n", b"\n")
        complete = streaming and re.search(rb"(?:^|\n)data: ?\[DONE\]\n\n", window)
        output.write(chunk)
        output.flush()
        if complete:
            return True
        tail = window[-64:]
    return not streaming


class LiveProvider(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.forward(None)

    def do_POST(self):
        length = int(self.headers.get('Content-Length', '0'))
        if not 0 < length <= 8_000_000:
            self.send_error(413)
            return
        self.forward(self.rfile.read(length))

    def forward(self, payload):
        observation = self.server.observation
        with observation.request():
            self.forward_observed(payload, observation)

    def forward_observed(self, payload, observation):
        if self.headers.get('Authorization') != 'Bearer ' + self.server.key:
            self.send_error(401)
            observation.failures.add('provider-auth')
            return
        if self.path not in ('/v1/models', '/v1/chat/completions'):
            self.send_error(404)
            observation.failures.add('provider-http')
            return
        request = urllib.request.Request('https://api.nan.builders' + self.path, data=payload,
                  headers={'Authorization': 'Bearer ' + self.server.key, 'Content-Type': 'application/json',
                           'User-Agent': 'nan-harness-canary'})
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                self.send_response(response.status)
                self.send_header('Content-Type', response.headers.get('Content-Type', 'application/json'))
                self.end_headers()
                if not forward_body(response, self.wfile):
                    observation.failures.add('provider-stream-incomplete')
                    return
            if payload:
                body = json.loads(payload)
                names = {tool['function']['name'] for tool in body.get('tools', [])}
                if 'subagent_run' in names:
                    observation.parent_seen = True
                elif 'read' in names and body.get('model') == self.server.model:
                    observation.child_seen = True
        except urllib.error.HTTPError as error:
            observation.failures.add({401: 'provider-auth', 403: 'provider-auth',
                                      402: 'provider-quota', 429: 'provider-throttled'}
                                     .get(error.code, 'provider-http'))
            self.send_error(error.code)
        except (OSError, ValueError, KeyError, TypeError, urllib.error.URLError):
            observation.failures.add('provider-transport')
