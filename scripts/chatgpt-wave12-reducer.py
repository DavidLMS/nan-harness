#!/usr/bin/env python3
"""Temporary wave-12 bounded startup-output reducer (Linux diagnostic only).

This is the in-memory half of the launch-only debug wrapper. It is invoked
either by ``chatgpt-wave12-shim.sh`` (the nanh shim, which has already
refused every non-launch argument shape) or directly by the test contracts.
It performs the actual observation:

  * it re-asserts the identity of the real nanh binary before spawning
    anything (the shim asserted it once; a second check covers direct
    invocation, so an unverified binary is never launched);
  * it spawns ``<real nanh> <launch argv...> --debug`` -- ``--debug`` is the
    pre-existing CLI flag (crates/nan-harness-cli/src/app/args/desktop.rs)
    that makes ``supervise_desktop`` (crates/nan-harness-cli/src/commands/
    chatgpt_desktop/process.rs) inherit the app's stdout/stderr instead of
    nulling them, so the application's own startup output flows into the
    pipes owned by this process;
  * it drains both pipes continuously until the launcher is reaped (never
    stopping the drain early, or a saturated pipe would wedge the child),
    bounds classified bytes, retained line length, line counts and wall
    time, and reduces everything to integers and closed vocabulary words
    *in memory*; two absolute deadlines bound every observation: a hard
    ceiling of deadline + grace + drain + slack, and -- once the launcher
    is reaped -- a post-exit drain ceiling that a detached descendant
    continuously writing into an inherited pipe cannot extend;
  * it preserves the child's exit disposition: it exits with the child's
    code, or re-raises the fatal signal so the parent observes the
    identical wait status; an external SIGTERM is forwarded as SIGTERM and
    an external SIGINT as SIGINT (they are not identical: nanh maps them
    to its own 143/130 cleanup exits, crates/nan-harness-runtime/src/
    signals.rs), exactly once so nanh runs its own cancellation cleanup,
    escalating to SIGKILL only after the grace window;
  * it writes one closed facts JSON (mode 0600 inside a 0700 directory) and
    never stores, prints, publishes or echoes any raw byte it read.
    If that write fails after a child ran, the child's disposition still
    wins: the publication failure surfaces as a missing facts file, which
    the job gate treats as failure, and never as a forged refusal status.

Privacy contract: the only strings this program emits are the vocabulary
constants below and fixed refusal sentences. Raw capture is never persisted:
no log file, no temp file, no stdout/stderr passthrough of child bytes.
Unknown text stays unknown; a truncated capture is reported as truncated and
cannot certify absence.

Signature allowlist (every token is grounded in an exact upstream source
string; nothing is invented from generic words):

  no-usable-sandbox            "No usable sandbox!"  -- Chromium
                               content/browser/zygote_host/
                               zygote_host_impl_linux.cc (LOG(FATAL))
  suid-sandbox-missing         "The SUID sandbox helper binary is missing"
                               -- Chromium sandbox/linux/suid/client/
                               setuid_sandbox_host.cc (LOG(FATAL))
  suid-sandbox-misconfigured   "The SUID sandbox helper binary was found"
                               -- same file; the message continues "but is
                                  not configured correctly"
  display-unavailable          "Missing X server or $DISPLAY" -- Chromium
                               ui/ozone/platform/x11/
                               ozone_platform_x11.cc (LOG(ERROR))
  loader-missing-shared-object "cannot open shared object file" -- glibc
                               elf/dl-load.c; the dynamic loader's fatal
                               "error while loading shared libraries" line
                               carries this substring

Only stderr is matched: every grounded string above is an stderr message
from the loader or from Chromium/Electron logging. stdout bytes and lines
are counted, never matched, and never retained.

Exit-status contract: an observation runs to its closed facts. Refusals
that happen before the facts directory is usable can only surface as the
shim's exit status, so three closed statuses are reserved: 78 (usage,
runtime or environment refusal, including a refused launch shape and a
refused facts write before anything spawned), 77 (the bound asserted for
the real binary does not match it), and 76 (the child could not be
spawned at all, an unexpected failure ended the observation, or a child
signal could not be reproduced). Once a child was spawned, only the
child's disposition is propagated (signal re-raise included), even when
publishing the facts fails: if the child exited with one of the reserved
codes itself, the facts are the authority and the code is still forwarded
unchanged, and a refused facts write leaves the child's disposition as the
status with the facts file simply absent.
"""

import errno
import fcntl
import hashlib
import json
import os
import re
import selectors
import signal
import stat
import subprocess
import sys
import time

EXIT_OK = 0
EXIT_REFUSED = 78
EXIT_IDENTITY = 77
EXIT_UNOBSERVABLE = 76

OBSERVATIONS = ("complete", "truncated", "cancelled", "timeout",
                "launch-failed", "observation-failed")
CLASSIFICATIONS = ("no-usable-sandbox", "suid-sandbox-missing",
                   "suid-sandbox-misconfigured", "display-unavailable",
                   "loader-missing-shared-object", "multiple-signatures",
                   "no-signature", "capture-incomplete", "observation-failed")
FAILURES = ("none", "identity-refused", "runtime-refused")
STOPS = ("none", "forwarded", "escalated-kill", "attempted")
DISPOSITIONS = ("exited", "signaled", "never-started")

# closed token -> exact stderr substring as printed upstream (matched on raw
# bytes: the capture is never decoded before classification)
SIGNATURES = (
    ("no-usable-sandbox", b"No usable sandbox!"),
    ("suid-sandbox-missing", b"The SUID sandbox helper binary is missing"),
    ("suid-sandbox-misconfigured", b"The SUID sandbox helper binary was found"),
    ("display-unavailable", b"Missing X server or $DISPLAY"),
    ("loader-missing-shared-object", b"cannot open shared object file"),
)
SIGNATURE_TOKENS = tuple(token for token, _ in SIGNATURES)

DEFAULT_MAX_BYTES = 1 << 20      # 1 MiB classified per stream, then drained
DEFAULT_MAX_LINE_BYTES = 8192    # retained bytes per line before overflow
DEFAULT_MAX_LINES = 65535        # closed-count ceiling per stream
DEFAULT_DEADLINE_S = 60          # startup observation window
DEFAULT_GRACE_S = 5              # wait after forwarding a stop before kill
POST_EXIT_DRAIN_S = 2            # linger for pipes the launcher no longer
                                 # owns: a detached descendant holding an
                                 # inherited write end must not pin the
                                 # observation past the launcher's life.
                                 # Absolute from the moment the launcher is
                                 # first observed dead; continuous writes
                                 # cannot extend it.
LOOP_SLACK_S = 1.0               # wake-up granularity margin for the hard
                                 # ceiling below
DIGEST_RE = re.compile(r"^[0-9a-f]{64}$")
ZERO64 = "0" * 64
SCHEMA_VERSION = 1


class Refusal(Exception):
    """A fail-closed refusal: nothing is spawned (or nothing more can be
    done), and only the closed refusal reason is recorded."""

    def __init__(self, reason, status=EXIT_REFUSED):
        super().__init__(reason)
        self.reason = reason
        self.status = status


def bounded_env(name, default, ceiling):
    raw = os.environ.get(name, "")
    if raw == "":
        return default
    if not re.fullmatch(r"[0-9]+", raw):
        raise Refusal("runtime-refused")
    value = int(raw, 10)
    if not 0 < value <= ceiling:
        raise Refusal("runtime-refused")
    return value


def file_digest(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        while True:
            chunk = handle.read(1 << 16)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def private_directory(path):
    try:
        info = os.lstat(path)
    except OSError as error:
        raise Refusal("runtime-refused") from error
    if not stat.S_ISDIR(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o700:
        # SECURITY.md private-directory mode, enforced not assumed.
        raise Refusal("runtime-refused")
    if info.st_uid != os.geteuid():
        # A 0700 directory owned by somebody else cannot be private to us;
        # lstat already refused a symlink, this refuses a foreign directory.
        raise Refusal("runtime-refused")
    return path


class Stream:
    """One non-blocking pipe drained line-wise with bounded retention.

    ``classify`` enables signature matching; stdout streams count only.
    Retention is one bounded partial-line buffer, regardless of volume.
    """

    def __init__(self, cap_bytes, cap_line_bytes, cap_lines, classify):
        self.cap_bytes = cap_bytes
        self.cap_line_bytes = cap_line_bytes
        self.cap_lines = cap_lines
        self.classify = classify
        self.bytes = 0
        self.classified = 0
        self.lines = 0
        self.unmatched = 0
        self.truncated = False
        self.matches = {token: 0 for token in SIGNATURE_TOKENS}
        self._buffer = b""
        self._line_open = False
        self._line_overflowed = False

    def feed(self, chunk):
        if not chunk:
            return
        self.bytes += len(chunk)
        if self.truncated:
            # Past every bound the content is dropped unread-but-counted:
            # the capture cannot certify what it stopped inspecting, and
            # the drain must continue so the child never blocks.
            return
        start = 0
        while start < len(chunk):
            if self.truncated:
                return
            if self.classified >= self.cap_bytes:
                # The classified-byte bound is consumed line by line, not
                # chunk by chunk: one huge read must not blind the
                # classifier to the grounded signatures at the head of
                # that read. Raw bytes past the bound stay counted, never
                # inspected.
                self.truncate_line()
                self.truncated = True
                return
            newline = chunk.find(b"\n", start)
            if newline < 0:
                self._absorb(chunk[start:])
                return
            self._absorb(chunk[start:newline])
            self._finish_line()
            start = newline + 1

    def _absorb(self, segment):
        if not segment:
            return
        self.classified += len(segment)
        if self.lines >= self.cap_lines:
            self.truncate_line()
            self.truncated = True
            return
        if not self._line_open:
            self.lines += 1
            self._line_open = True
            self._line_overflowed = False
            self._buffer = b""
        room = self.cap_line_bytes - len(self._buffer)
        if len(segment) > room:
            self._buffer += segment[:room]
            self._line_overflowed = True
        else:
            self._buffer += segment

    def _finish_line(self):
        if not self._line_open:
            return
        line = self._buffer
        self.truncate_line()
        self._buffer = b""
        self._line_open = False
        if not self.classify:
            self.unmatched += 1
            return
        matched = False
        if not self._line_overflowed:
            for token, needle in SIGNATURES:
                if needle in line:
                    self.matches[token] += 1
                    matched = True
        else:
            # An over-long line is unknown text: counted, never matched,
            # and the fact that classification saw only its head is a
            # truncation of the capture.
            self.truncated = True
        if not matched:
            self.unmatched += 1

    def truncate_line(self):
        self._line_open = False
        self._buffer = b""

    def finish_open_line(self):
        self._finish_line()


def _set_nonblocking(fd):
    flags = fcntl.fcntl(fd, fcntl.F_GETFL)
    fcntl.fcntl(fd, fcntl.F_SETFL, flags | os.O_NONBLOCK)


class Observation:
    def __init__(self):
        self.observation = "observation-failed"
        self.disposition = "never-started"
        self.code = -1
        self.number = -1
        self.stop = "none"


def observe(argv, deadline_s, grace_s, cap_bytes, cap_line_bytes, cap_lines):
    """Spawn the debug launch, drain both streams, return the observation
    and the two reduced streams. Nothing spawns unless the argv was built
    by do_observe after its identity assertions."""
    observation = Observation()

    external = []

    def on_stop(signum, _frame):
        external.append(signum)

    # The stop handlers go in place BEFORE the spawn: from the moment a
    # child could exist, this process can already absorb a cancellation
    # instead of dying from it with the facts unwritten.
    previous = {}
    for signum in (signal.SIGTERM, signal.SIGINT):
        try:
            previous[signum] = signal.signal(signum, on_stop)
        except ValueError:
            pass  # not the main thread: the deadline still bounds observation
    try:
        child = subprocess.Popen(
            argv, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except OSError:
        for signum, handler in previous.items():
            signal.signal(signum, handler)
        observation.observation = "launch-failed"
        return observation, Stream(0, 0, 0, False), Stream(0, 0, 0, True)

    out = Stream(cap_bytes, cap_line_bytes, cap_lines, False)
    err = Stream(cap_bytes, cap_line_bytes, cap_lines, True)
    fds = {child.stdout.fileno(): out, child.stderr.fileno(): err}
    for fd in fds:
        _set_nonblocking(fd)
    selector = selectors.DefaultSelector()
    for fd in fds:
        selector.register(fd, selectors.EVENT_READ)
    open_fds = set(fds)

    start = time.monotonic()
    # Absolute ceilings. Nothing inside the loop can push either one later:
    # a stop forwarded to a deaf launcher, or a detached descendant writing
    # into an inherited pipe forever, ends the observation at the ceiling
    # with a closed incomplete-capture fact, never at an unbounded wait.
    hard_deadline = start + deadline_s + grace_s + POST_EXIT_DRAIN_S \
        + LOOP_SLACK_S
    term_sent = False
    term_at = None
    reaped_at = None
    cause = None  # "external" or "deadline", whichever ended the window
    try:
        while True:
            now = time.monotonic()
            if reaped_at is None and child.poll() is not None:
                reaped_at = now
            stop_needed = bool(external) or now - start >= deadline_s
            if stop_needed and cause is None:
                cause = "external" if external else "deadline"
            if stop_needed and not term_sent:
                if reaped_at is not None:
                    # The launcher is already gone: there is nothing to
                    # stop, and claiming a forward would be a lie.
                    observation.stop = "attempted"
                else:
                    # Forward the stop exactly once, as the same signal:
                    # SIGINT stays SIGINT and SIGTERM stays SIGTERM, so
                    # nanh runs the matching cleanup path (130 vs 143).
                    signum = external[0] if external else signal.SIGTERM
                    try:
                        child.send_signal(signum)
                        observation.stop = "forwarded"
                    except OSError:
                        observation.stop = "attempted"
                term_sent = True
                term_at = now
            elif term_sent and observation.stop == "forwarded" \
                    and reaped_at is None and now - term_at > grace_s:
                # nanh ignored or could not finish its cancellation within
                # the grace window: escalate so no process this observation
                # directly owns survives the temporary job. Descendants the
                # launcher detached are not this observation's to kill.
                try:
                    child.kill()
                except OSError:
                    pass
                observation.stop = "escalated-kill"
            ceiling = hard_deadline
            if reaped_at is not None:
                ceiling = min(ceiling, reaped_at + POST_EXIT_DRAIN_S)
                if not open_fds:
                    break  # launcher reaped, every pipe read to EOF
            if now >= ceiling:
                break
            wait = 0.05
            if external or now + wait > ceiling:
                wait = max(min(wait, ceiling - now), 0.001)
            try:
                events = selector.select(timeout=wait)
            except OSError:
                events = []
            for key, _mask in events:
                try:
                    chunk = os.read(key.fd, 1 << 16)
                except BlockingIOError:
                    continue
                except OSError:
                    chunk = b""
                if chunk:
                    fds[key.fd].feed(chunk)
                else:
                    # EOF on a pipe read end is permanent once reported:
                    # stop polling that end for good -- a read end at EOF
                    # stays "readable" forever, so leaving it registered
                    # would spin the loop at full CPU. The other end, the
                    # launcher's life and the ceilings above decide when
                    # the loop ends.
                    try:
                        selector.unregister(key.fd)
                    except Exception:
                        pass
                    open_fds.discard(key.fd)
        out.finish_open_line()
        err.finish_open_line()
        # The direct child is this observation's to reap. If a ceiling
        # elapsed while it somehow still lived (escalated SIGKILL is
        # unblockable, so realistically only a failed kill can reach
        # here), one last kill precedes the reap; a foreign descendant is
        # never touched.
        if child.poll() is None:
            try:
                child.kill()
            except OSError:
                pass
        # Reaping happens while the stop handlers are still installed: a
        # signal arriving in this window must not kill the reducer before
        # the child's disposition has been recorded and preserved.
        status = child.wait()
    finally:
        for fd in list(open_fds):
            try:
                selector.unregister(fd)
            except Exception:
                pass
        for signum, handler in previous.items():
            signal.signal(signum, handler)
    selector.close()
    child.stdout.close()
    child.stderr.close()

    if status >= 0:
        observation.disposition = "exited"
        observation.code = status
    else:
        observation.disposition = "signaled"
        observation.number = -status
    if cause == "deadline":
        observation.observation = "timeout"
    elif cause == "external":
        observation.observation = "cancelled"
    elif open_fds or out.truncated or err.truncated:
        # Pipes still open at the ceiling mean bytes the launcher's
        # descendants were still writing went unread: an incomplete
        # capture, reported as such.
        observation.observation = "truncated"
    else:
        observation.observation = "complete"
    return observation, out, err


def classify(observation, err):
    matched = sorted(token for token in SIGNATURE_TOKENS if err.matches[token] > 0)
    if len(matched) > 1:
        return "multiple-signatures"
    if len(matched) == 1:
        # A positive match is evidence even inside a truncated or cancelled
        # window: the bytes that proved the signature were really seen.
        return matched[0]
    if observation in ("timeout", "cancelled", "launch-failed",
                       "observation-failed"):
        return "observation-failed"
    if observation == "truncated":
        # A truncated capture cannot certify that no signature was present.
        return "capture-incomplete"
    return "no-signature"


def build_facts(bounds, digests, observation, out, err):
    return {
        "schemaVersion": SCHEMA_VERSION,
        "observation": observation.observation,
        "classification": classify(observation.observation, err),
        "failure": "none",
        "launcherExit": observation.code,
        "launcherDisposition": observation.disposition,
        "launcherSignal": observation.number,
        "stopAction": observation.stop,
        "stdoutBytes": min(out.bytes, bounds["maxStreamBytes"]),
        "stderrBytes": min(err.bytes, bounds["maxStreamBytes"]),
        "stdoutTruncated": 1 if out.truncated else 0,
        "stderrTruncated": 1 if err.truncated else 0,
        "stdoutLines": min(out.lines, bounds["maxLines"]),
        "stderrLines": min(err.lines, bounds["maxLines"]),
        "unmatchedStdoutLines": min(out.unmatched, bounds["maxLines"]),
        "unmatchedStderrLines": min(err.unmatched, bounds["maxLines"]),
        "signatures": {token: min(err.matches[token], bounds["maxLines"])
                       for token in SIGNATURE_TOKENS},
        "bounds": dict(bounds),
        "identity": dict(digests),
    }


def refusal_facts(reason, bounds, real_digest):
    return {
        "schemaVersion": SCHEMA_VERSION,
        "observation": "observation-failed",
        "classification": "observation-failed",
        "failure": reason,
        "launcherExit": -1,
        "launcherDisposition": "never-started",
        "launcherSignal": -1,
        "stopAction": "none",
        "stdoutBytes": 0, "stderrBytes": 0,
        "stdoutTruncated": 0, "stderrTruncated": 0,
        "stdoutLines": 0, "stderrLines": 0,
        "unmatchedStdoutLines": 0, "unmatchedStderrLines": 0,
        "signatures": {token: 0 for token in SIGNATURE_TOKENS},
        "bounds": dict(bounds),
        "identity": {
            # A refusal never claims a measured digest: the expected one is
            # only published when it is the identity that was asserted, and
            # an identity refusal redacts it to zeros because nothing about
            # the found file is trusted.
            "realNanhSha256": ZERO64 if reason == "identity-refused"
            else (real_digest or ZERO64),
            "shimSha256": ZERO64,
            "reducerSha256": ZERO64,
        },
    }


def write_facts(directory, facts):
    payload = json.dumps(facts, separators=(",", ":"), sort_keys=True)
    if validate_facts(facts) is not None:
        # A facts document this program cannot validate is not evidence;
        # refusing to publish beats publishing a broken claim.
        raise Refusal("runtime-refused")
    temp = os.path.join(directory, ".startup-facts.json.tmp")
    final = os.path.join(directory, "startup-facts.json")
    try:
        os.unlink(temp)
    except OSError as error:
        if error.errno != errno.ENOENT:
            raise Refusal("runtime-refused") from error
    try:
        handle = os.open(temp, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(handle, "w", encoding="utf-8") as stream:
            stream.write(payload + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temp, 0o600)
        os.replace(temp, final)
    except OSError as error:
        raise Refusal("runtime-refused") from error
    return final


def validate_facts(facts):
    """The strict closed contract of a wave12 facts document: exact keys,
    closed vocabularies, bounded integers, and cross-field rules that refuse
    a claim the counts do not show. Returns None or a refusal string."""

    def integer(value, low, high):
        return (isinstance(value, int) and not isinstance(value, bool)
                and low <= value <= high)

    if not isinstance(facts, dict):
        return "the facts are not an object"
    if facts.get("schemaVersion") != SCHEMA_VERSION:
        return "the facts are not version 1"
    exact = {"schemaVersion", "observation", "classification", "failure",
             "launcherExit", "launcherDisposition", "launcherSignal",
             "stopAction", "stdoutBytes", "stderrBytes", "stdoutTruncated",
             "stderrTruncated", "stdoutLines", "stderrLines",
             "unmatchedStdoutLines", "unmatchedStderrLines", "signatures",
             "bounds", "identity"}
    if set(facts) != exact:
        return "the facts keys are not closed"
    if facts["observation"] not in OBSERVATIONS:
        return "the observation is not a closed word"
    if facts["classification"] not in CLASSIFICATIONS:
        return "the classification is not a closed word"
    if facts["failure"] not in FAILURES:
        return "the failure is not a closed word"
    if facts["launcherDisposition"] not in DISPOSITIONS:
        return "the disposition is not a closed word"
    if facts["stopAction"] not in STOPS:
        return "the stop action is not a closed word"
    if not integer(facts["launcherExit"], -1, 255):
        return "the launcher exit is not a bounded status"
    if not integer(facts["launcherSignal"], -1, 64):
        return "the launcher signal is not bounded"
    for name in ("stdoutBytes", "stderrBytes"):
        if not integer(facts[name], 0, 8 << 20):
            return "a byte count is not bounded"
    for name in ("stdoutTruncated", "stderrTruncated"):
        if not integer(facts[name], 0, 1):
            return "a truncation flag is not closed"
    for name in ("stdoutLines", "stderrLines", "unmatchedStdoutLines",
                 "unmatchedStderrLines"):
        if not integer(facts[name], 0, 65535):
            return "a line count is not bounded"
    bounds = facts["bounds"]
    if not isinstance(bounds, dict) or set(bounds) != {
            "maxStreamBytes", "maxLineBytes", "maxLines", "deadlineSeconds",
            "graceSeconds"}:
        return "the bounds keys are not closed"
    if not integer(bounds["maxStreamBytes"], 1, 8 << 20):
        return "the byte bound is not bounded"
    if not integer(bounds["maxLineBytes"], 1, 65536):
        return "the line bound is not bounded"
    if not integer(bounds["maxLines"], 1, 65535):
        return "the line-count bound is not bounded"
    if not integer(bounds["deadlineSeconds"], 1, 600):
        return "the deadline bound is not bounded"
    if not integer(bounds["graceSeconds"], 1, 30):
        return "the grace bound is not bounded"
    identity = facts["identity"]
    if not isinstance(identity, dict) or set(identity) != {
            "realNanhSha256", "shimSha256", "reducerSha256"}:
        return "the identity keys are not closed"
    for value in identity.values():
        if not isinstance(value, str) or not DIGEST_RE.match(value):
            return "an identity digest is not a digest"
    signatures = facts["signatures"]
    if not isinstance(signatures, dict) or set(signatures) != set(SIGNATURE_TOKENS):
        return "the signature keys are not closed"
    total = 0
    for token in SIGNATURE_TOKENS:
        if not integer(signatures[token], 0, 65535):
            return "a signature count is not bounded"
        total += signatures[token]
    # Cross-field rules: the classification must be a function of the
    # published counts, so a run cannot claim more than it observed.
    matched = [token for token in SIGNATURE_TOKENS if signatures[token] > 0]
    if facts["failure"] != "none":
        if facts["observation"] != "observation-failed":
            return "a refusal still reported an observation"
        if facts["classification"] != "observation-failed":
            return "a refusal classified a cause"
        if total or facts["launcherDisposition"] != "never-started":
            return "a refusal spawned something"
    elif facts["launcherDisposition"] == "never-started" and \
            facts["observation"] != "launch-failed":
        return "a non-refusal never started without a launch failure"
    if facts["observation"] == "complete" and (
            facts["stdoutTruncated"] or facts["stderrTruncated"]):
        return "a complete capture claims truncation"
    if facts["observation"] in ("timeout", "cancelled") and \
            facts["stopAction"] == "none":
        return "an ended observation stopped nothing"
    if facts["observation"] in ("launch-failed", "observation-failed") and \
            facts["failure"] == "none" and \
            facts["classification"] != "observation-failed":
        return "a failed observation classified a cause"
    if len(matched) > 1 and facts["classification"] != "multiple-signatures":
        return "several signatures matched without saying so"
    if len(matched) == 1 and facts["classification"] != matched[0]:
        return "the classification does not name the matched signature"
    if len(matched) == 0:
        if facts["classification"] in SIGNATURE_TOKENS or \
                facts["classification"] == "multiple-signatures":
            return "a cause was classified without a matching signature"
        if facts["classification"] == "no-signature" and (
                facts["observation"] != "complete"
                or facts["stdoutTruncated"] or facts["stderrTruncated"]):
            return "absence was certified from an incomplete capture"
        if facts["classification"] == "capture-incomplete" and (
                facts["observation"] != "truncated"):
            return "capture-incomplete without a truncated capture"
    if total > facts["stderrLines"]:
        return "more signatures matched than lines were seen"
    return None


def load_facts_file(path):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            facts = json.load(handle)
    except (OSError, ValueError):
        return None, "the facts file is not readable JSON"
    return facts, None


def do_validate(argv):
    if len(argv) != 2 or argv[0] != "--facts":
        return EXIT_REFUSED
    facts, problem = load_facts_file(argv[1])
    if problem is None:
        problem = validate_facts(facts)
    if problem is not None:
        sys.stderr.write("wave12 reducer: the facts were refused\n")
        return EXIT_REFUSED
    sys.stderr.write("wave12 reducer: the facts were validated\n")
    return EXIT_OK


TRANSPARENT_FLAGS = ("--version", "--help", "--restore", "--dry-run")


def parse_observe_args(argv):
    options = {"real": None, "sha256": None, "facts": None, "shim": None}
    index = 0
    while index < len(argv) and argv[index] != "--":
        key = argv[index]
        if key.startswith("--") and key[2:] in options \
                and index + 1 < len(argv) and options[key[2:]] is None:
            options[key[2:]] = argv[index + 1]
            index += 2
        else:
            raise Refusal("runtime-refused")
    if index >= len(argv) or argv[index] != "--" or not argv[index + 1:]:
        raise Refusal("runtime-refused")
    launch = list(argv[index + 1:])
    if options["real"] is None or options["sha256"] is None \
            or options["facts"] is None or options["shim"] is None:
        raise Refusal("runtime-refused")
    if not options["real"].startswith("/") or not options["facts"].startswith("/"):
        raise Refusal("runtime-refused")
    if not DIGEST_RE.match(options["sha256"]) or not options["shim"].startswith("/"):
        raise Refusal("runtime-refused")
    # The reducer only ever sees the exact seven-word launch vector the
    # checker builds (probe.rs launch_command); anything else means the
    # shim routed wrong, and guessing here would change normal CLI
    # behavior. The two flag positions are pinned so a value can never
    # masquerade as a flag or shift the shape.
    exact = (len(launch) == 7 and launch[0] == "chatgpt-desktop"
             and launch[1] == "--provider-base-url"
             and launch[3] == "--model" and launch[5] == "--executable")
    if not exact:
        raise Refusal("runtime-refused")
    for word in launch[2::2]:
        if word.startswith("-"):
            raise Refusal("runtime-refused")
    for flag in TRANSPARENT_FLAGS + ("--debug",):
        if flag in launch:
            raise Refusal("runtime-refused")
    return options, launch


def do_observe(argv):
    options, launch = parse_observe_args(argv)
    bounds = {
        "maxStreamBytes": bounded_env("WAVE12_MAX_BYTES", DEFAULT_MAX_BYTES,
                                      8 << 20),
        "maxLineBytes": bounded_env("WAVE12_MAX_LINE_BYTES",
                                    DEFAULT_MAX_LINE_BYTES, 65536),
        "maxLines": bounded_env("WAVE12_MAX_LINES", DEFAULT_MAX_LINES, 65535),
        "deadlineSeconds": bounded_env("WAVE12_DEADLINE_S", DEFAULT_DEADLINE_S,
                                       600),
        "graceSeconds": bounded_env("WAVE12_GRACE_S", DEFAULT_GRACE_S, 30),
    }
    # A refused argument shape must leave the previous observation intact,
    # so nothing (not even a refusal fact) is written until every bound is
    # known good.
    directory = private_directory(options["facts"])
    real = options["real"]
    identity_status = EXIT_IDENTITY
    try:
        info = os.lstat(real)
        if not stat.S_ISREG(info.st_mode) or not os.access(real, os.X_OK):
            raise OSError()
    except OSError:
        write_facts(directory, refusal_facts("identity-refused", bounds, None))
        raise Refusal("identity-refused", identity_status) from None
    try:
        mismatch = file_digest(real) != options["sha256"]
    except OSError:
        mismatch = True
    if mismatch:
        # The found file is not the binary this observation bound itself to:
        # nothing is launched, and no digest of it is published.
        write_facts(directory, refusal_facts("identity-refused", bounds, None))
        raise Refusal("identity-refused", identity_status)
    reducer_path = os.path.realpath(__file__)
    shim_path = os.path.realpath(options["shim"])
    if not os.path.isfile(shim_path):
        write_facts(directory, refusal_facts("runtime-refused", bounds,
                                             options["sha256"]))
        raise Refusal("runtime-refused")
    try:
        digests = {
            "realNanhSha256": options["sha256"],
            "shimSha256": file_digest(shim_path),
            "reducerSha256": file_digest(reducer_path),
        }
    except OSError:
        write_facts(directory, refusal_facts("runtime-refused", bounds,
                                             options["sha256"]))
        raise Refusal("runtime-refused") from None
    # Source binding: the three digests published with every observation are
    # measured from the bytes present at observation time -- the real binary
    # (asserted equal to the caller's bound digest above), the wrapper that
    # routed this launch, and this reducer. The real binary's digest is the
    # only one that names the product: the wrapper documents what it wraps,
    # it never substitutes itself for the checked identity.
    # Preflight the exact document shape against the same strict contract
    # the future job will use to gate its upload, so a schema this program
    # cannot validate is never produced at observation time.
    probe = Observation()
    probe.observation = "launch-failed"
    if validate_facts(build_facts(bounds, digests, probe,
                                  Stream(0, 0, 0, False),
                                  Stream(0, 0, 0, True))) is not None:
        write_facts(directory, refusal_facts("runtime-refused", bounds,
                                             options["sha256"]))
        raise Refusal("runtime-refused")

    # The exact ChatGPT launch, plus the pre-existing --debug flag and
    # nothing else: no invented flag, no changed routing or ownership
    # argument, no weakened executable or version check.
    observation, out, err = observe(
        [real] + launch + ["--debug"], bounds["deadlineSeconds"],
        bounds["graceSeconds"], bounds["maxStreamBytes"],
        bounds["maxLineBytes"], bounds["maxLines"])
    facts = build_facts(bounds, digests, observation, out, err)
    try:
        write_facts(directory, facts)
    except Refusal:
        # A facts document that cannot be published must not overwrite the
        # truth of a child that ran: the disposition below is still what
        # this process propagates, and the missing file is what makes the
        # future job's upload gate fail closed. Pre-spawn refusals keep
        # their reserved statuses; after a spawn the child outranks us.
        pass
    if observation.disposition == "signaled":
        number = observation.number
        # Reproduce the fatal signal on this process so the caller sees
        # the same wait status. Only signals this reducer caught (TERM
        # and INT) need their disposition reset; any other fatal signal
        # (an escalation's SIGKILL, a child's SIGSEGV) is already at its
        # default action, and asking to reset SIGKILL itself is an error.
        caught = {int(signal.SIGTERM), int(signal.SIGINT)}
        try:
            if number in caught:
                signal.signal(number, signal.SIG_DFL)
            os.kill(os.getpid(), number)
        except (OSError, ValueError):
            pass
        # Reaching here means the re-raise did not take: this process
        # cannot honestly claim the child's fatal-signal status as its
        # own, so the reserved unobservable status says so instead.
        return EXIT_UNOBSERVABLE
    if observation.disposition == "never-started":
        # launch-failed: no child status exists to propagate.
        return EXIT_UNOBSERVABLE
    return observation.code


def do_refuse(argv):
    """Record a closed preflight refusal from the shim, for the cases where
    the refusal happens before (or instead of) any spawn: the reducer never
    trusts the caller's reason, only its own vocabulary."""
    if len(argv) != 4 or argv[0] != "--facts" or argv[2] != "--reason":
        return EXIT_REFUSED
    reason = {"identity": "identity-refused",
              "runtime": "runtime-refused"}.get(argv[3])
    if reason is None:
        return EXIT_REFUSED
    bounds = {
        "maxStreamBytes": bounded_env("WAVE12_MAX_BYTES", DEFAULT_MAX_BYTES,
                                      8 << 20),
        "maxLineBytes": bounded_env("WAVE12_MAX_LINE_BYTES",
                                    DEFAULT_MAX_LINE_BYTES, 65536),
        "maxLines": bounded_env("WAVE12_MAX_LINES", DEFAULT_MAX_LINES, 65535),
        "deadlineSeconds": bounded_env("WAVE12_DEADLINE_S", DEFAULT_DEADLINE_S,
                                       600),
        "graceSeconds": bounded_env("WAVE12_GRACE_S", DEFAULT_GRACE_S, 30),
    }
    directory = private_directory(argv[1])
    write_facts(directory, refusal_facts(reason, bounds, None))
    return EXIT_OK


def usage():
    sys.stderr.write(
        "usage: chatgpt-wave12-reducer.py observe --real <path> --sha256 <hex>"
        " --shim <path> --facts <dir> -- <launch argv...>\n"
        "       chatgpt-wave12-reducer.py validate --facts <file>\n"
        "       chatgpt-wave12-reducer.py refuse --facts <dir>"
        " --reason identity|runtime\n")
    return EXIT_REFUSED


def main():
    sys.stdout = open(os.devnull, "w")
    sys.stderr = open(os.devnull, "w")
    argv = sys.argv[1:]
    if not argv:
        return EXIT_REFUSED
    try:
        if argv[0] == "validate":
            return do_validate(argv[1:])
        if argv[0] == "refuse":
            return do_refuse(argv[1:])
        if argv[0] == "observe":
            return do_observe(argv[1:])
        return EXIT_REFUSED
    except Refusal as refusal:
        return refusal.status
    except Exception:  # noqa: BLE001 -- fail closed, never echo a traceback
        return EXIT_UNOBSERVABLE


if __name__ == "__main__":
    try:
        code = main()
    except Exception:  # noqa: BLE001
        code = EXIT_UNOBSERVABLE
    sys.stdout.flush()
    sys.stderr.flush()
    os._exit(code)
