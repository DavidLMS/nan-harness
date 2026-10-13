"""Observe live parent/child routing without retaining model traffic."""
import http.server
import json
import urllib.error
import urllib.request


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
        if self.headers.get('Authorization') != 'Bearer ' + self.server.key:
            self.send_error(401)
            return
        if self.path not in ('/v1/models', '/v1/chat/completions'):
            self.send_error(404)
            return
        request = urllib.request.Request('https://api.nan.builders' + self.path, data=payload,
                  headers={'Authorization': 'Bearer ' + self.server.key, 'Content-Type': 'application/json',
                           'User-Agent': 'nan-harness-canary'})
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                self.send_response(response.status)
                self.send_header('Content-Type', response.headers.get('Content-Type', 'application/json'))
                self.end_headers()
                while chunk := response.read1(65536):
                    self.wfile.write(chunk)
                    self.wfile.flush()
            if payload:
                body = json.loads(payload)
                names = {tool['function']['name'] for tool in body.get('tools', [])}
                if 'subagent_run' in names:
                    self.server.parent_seen = True
                elif 'read' in names and body.get('model') == self.server.model:
                    self.server.child_seen = True
        except (OSError, ValueError, urllib.error.URLError):
            self.server.failed = True
