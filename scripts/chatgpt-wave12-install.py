#!/usr/bin/env python3
"""Install the pinned package only on a disposable GitHub Linux runner.

The private checker installer intentionally does not run package maintainer
scripts. This experiment instead uses the publisher's system installation and
exact AppArmor attachment path, without relaxing the host namespace policy.
"""

import hashlib
import json
import os
from pathlib import Path
import platform
import selectors
import signal
import stat
import subprocess
import sys
import time


VERSION = "26.908.40834"
PACKAGE_SHA256 = "da37b8e7bcefaaea019c478cacbe6c73ee1ddd15e0e1ebb3c7ef0a42dd818ac2"
PACKAGE_URL = ("https://persistent.oaistatic.com/codex-app-prod/linux/deb/"
               f"pool/main/c/chatgpt/chatgpt_{VERSION}_amd64.deb")
EXECUTABLE = Path("/usr/lib/chatgpt/ChatGPT")
PROFILE = Path("/etc/apparmor.d/chatgpt")
RESTRICTION = Path("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
MAX_COMMAND_OUTPUT = 64 * 1024
MAX_RESTRICTION_BYTES = 32
MAX_PROFILE_BYTES = 16 * 1024
MAX_JOURNAL_BYTES = 32 * 1024
CUSTOM_PROFILE = ('abi <abi/4.0>,\ninclude <tunables/global>\n\n'
                  'profile chatgpt "/usr/lib/chatgpt/ChatGPT" flags=(unconfined) {\n'
                  '  userns,\n}\n')


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def restriction_value():
    """Read the single policy value without unbounded file reads."""
    with RESTRICTION.open("rb") as source:
        data = source.read(MAX_RESTRICTION_BYTES + 1)
    require(len(data) <= MAX_RESTRICTION_BYTES, "namespace restriction is too large")
    return data.decode("ascii").strip()


def profile_text():
    with PROFILE.open("rb") as source:
        data = source.read(MAX_PROFILE_BYTES + 1)
    require(len(data) <= MAX_PROFILE_BYTES, "AppArmor profile is too large")
    return data.decode("ascii")


def validate_profile():
    text = profile_text()
    require('profile chatgpt "/usr/lib/chatgpt/ChatGPT"' in text
            and "userns," in text,
            "AppArmor profile does not attach the expected executable")
    return text


def write_journal(directory, state, **updates):
    journal = directory / "sandbox-journal.json"
    value = {**state, **updates}
    encoded = json.dumps(value, sort_keys=True).encode("ascii")
    require(len(encoded) <= MAX_JOURNAL_BYTES, "sandbox journal is too large")
    temporary = directory / "sandbox-journal.pending"
    temporary.write_bytes(encoded + b"\n")
    temporary.replace(journal)
    return value


def bounded_json(path):
    require(path.is_file() and not path.is_symlink(), "checker cleanup evidence is missing")
    with path.open("rb") as source:
        raw = source.read(MAX_JOURNAL_BYTES + 1)
    require(len(raw) <= MAX_JOURNAL_BYTES, "checker cleanup evidence is too large")
    try:
        value = json.loads(raw)
    except (ValueError, UnicodeError):
        raise ValueError("checker cleanup evidence is invalid") from None
    require(type(value) is dict, "checker cleanup evidence is invalid")
    return value


def validate_checker_report(report):
    """Accept only the desktop-suite report shape that proves cleanup."""
    require(set(report) <= {"schemaVersion", "checkerVersion", "runId", "startedAt", "platform",
                            "architecture", "model", "nanHarness", "results", "cleanup"},
            "checker report fields are invalid")
    require({"schemaVersion", "checkerVersion", "runId", "startedAt", "platform", "architecture",
             "results", "cleanup"} <= set(report), "checker report fields are incomplete")
    require(report.get("schemaVersion") == 3
            and report.get("platform") == "linux"
            and report.get("architecture") == "x86_64"
            and report.get("cleanup") == "passed", "checker report identity is invalid")
    results = report.get("results")
    require(type(results) is list and len(results) == 1 and type(results[0]) is dict,
            "checker report results are invalid")
    require(results[0].get("app") == "chatgpt-desktop"
            and results[0].get("cleanup") == "passed",
            "checker app cleanup is unconfirmed")


def command(arguments, timeout=30, accepted=(0,)):
    require(timeout > 0, "runner command timeout must be positive")
    process = subprocess.Popen(arguments, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    selector = None
    streams = {}

    def stop_group():
        if process.poll() is None:
            try:
                process.kill()
            except ProcessLookupError:
                pass
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
        try:
            process.wait(timeout=1)
        except subprocess.TimeoutExpired:
            raise ValueError("runner process did not stop") from None

    def close_streams():
        if selector is not None:
            for key in list(selector.get_map().values()):
                try:
                    selector.unregister(key.fileobj)
                except (KeyError, OSError):
                    pass
                try:
                    key.fileobj.close()
                except OSError:
                    pass
            try:
                selector.close()
            except OSError:
                pass
        for stream in (process.stdout, process.stderr):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass

    try:
        selector = selectors.DefaultSelector()
        stdout_fd = process.stdout.fileno()
        streams = {stdout_fd: bytearray(), process.stderr.fileno(): bytearray()}
        for stream in (process.stdout, process.stderr):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ)
        deadline = time.monotonic() + timeout
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                stop_group()
                raise subprocess.TimeoutExpired(arguments, timeout)
            events = selector.select(remaining)
            if not events:
                stop_group()
                raise subprocess.TimeoutExpired(arguments, timeout)
            for key, _ in events:
                fd = key.fileobj.fileno()
                chunk = os.read(fd, 4096)
                if not chunk:
                    selector.unregister(key.fileobj)
                    key.fileobj.close()
                    continue
                captured = streams[fd]
                if len(captured) <= MAX_COMMAND_OUTPUT:
                    captured.extend(chunk[:MAX_COMMAND_OUTPUT + 1 - len(captured)])
                if len(captured) > MAX_COMMAND_OUTPUT:
                    stop_group()
                    raise ValueError("runner command output too large")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            stop_group()
            raise subprocess.TimeoutExpired(arguments, timeout)
        return_code = process.wait(timeout=remaining)
    except (OSError, subprocess.TimeoutExpired, ValueError):
        stop_group()
        raise
    finally:
        close_streams()
    require(return_code in accepted, "runner command failed")
    return bytes(streams[stdout_fd]).decode("utf-8").strip()


def package_profile_state(package):
    """Inspect the package manifest before deciding whether a profile is needed."""
    listing = command(["dpkg-deb", "--contents", str(package)])
    entries = {parts[-1] for line in listing.splitlines()
               if (parts := line.split())}
    return ("package-profile-present" if "./etc/apparmor.d/chatgpt" in entries
            else "package-profile-absent")


def profile_loaded():
    profiles = command(["sudo", "-n", "cat", "/sys/kernel/security/apparmor/profiles"])
    return any(line in ("chatgpt (unconfined)", "chatgpt (enforce)")
               for line in profiles.splitlines())


def profile_attachment_observation():
    """Report only the profile listing; this is not proof of process attachment."""
    return "listed" if profile_loaded() else "not-listed"


def check_runner():
    expected = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
                "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64"}
    require(all(os.environ.get(key) == value for key, value in expected.items())
            and platform.system() == "Linux" and platform.machine() == "x86_64"
            and os.getuid() != 0, "disposable Linux x64 runner required")
    require(not any(key in os.environ for key in
                    ("NAN_API_KEY", "OPENAI_API_KEY", "CODEX_API_KEY")),
            "remove provider credentials before installation")
    require(restriction_value() == "1", "namespace restriction must remain enabled")
    command(["sudo", "-n", "aa-enabled", "--quiet"])
    require(not profile_loaded(), "an existing ChatGPT profile must be preserved")
    for path in (EXECUTABLE.parent, Path("/usr/bin/chatgpt"), PROFILE,
                 Path("/etc/apparmor.d/local/chatgpt"), Path("/etc/apparmor.d/disable/chatgpt"),
                 Path("/etc/default/chatgpt"), Path("/var/lib/chatgpt"),
                 Path("/etc/apt/sources.list.d/chatgpt.sources"),
                 Path("/usr/share/keyrings/chatgpt-archive-keyring.gpg")):
        require(not path.exists() and not path.is_symlink(),
                "an existing ChatGPT installation must be preserved")
    command(["dpkg-query", "--show", "--showformat=${Status}", "chatgpt"], accepted=(1,))


def digest(path):
    with path.open("rb") as source:
        checksum = hashlib.sha256()
        while chunk := source.read(65536):
            checksum.update(chunk)
    return checksum.hexdigest()


def root_owned_file(path):
    require(path.is_file(), "installed file is missing")
    for component in (path, *path.parents):
        metadata = component.lstat()
        require(metadata.st_uid == 0 and not metadata.st_mode & 0o022
                and not stat.S_ISLNK(metadata.st_mode), "installed path is not root-owned")


def install(directory):
    check_runner()
    require(directory.is_absolute() and directory.parent.is_dir()
            and not directory.parent.is_symlink(), "private runner directory required")
    directory.mkdir(mode=0o700)
    journal = {"schemaVersion": 1, "kind": "chatgpt-sandbox-journal",
               "phase": "created", "packageSha256": PACKAGE_SHA256,
               "packageVersion": VERSION, "createdPackage": False,
               "createdProfile": False, "profileSha256": "unknown",
               "executableSha256": "unknown",
               "launchState": "prelaunch"}
    write_journal(directory, journal)
    package = directory / "chatgpt.deb"
    journal = write_journal(directory, journal, phase="download-started")
    command(["curl", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https",
             "--max-time", "300", "--max-filesize", "450000000", "--silent", "--show-error",
             "--output", str(package), PACKAGE_URL], timeout=310)
    require(digest(package) == PACKAGE_SHA256, "official package digest mismatch")
    journal = write_journal(directory, journal, phase="package-verified", createdPackage=True)
    for field, expected in (("Package", "chatgpt"), ("Version", VERSION), ("Architecture", "amd64")):
        require(command(["dpkg-deb", "--field", str(package), field]) == expected,
                "official package identity mismatch")
    package_profile = package_profile_state(package)
    expected_profile = (hashlib.sha256(CUSTOM_PROFILE.encode("ascii")).hexdigest()
                        if package_profile == "package-profile-absent" else "unknown")
    journal = write_journal(directory, journal, phase="install-started",
                            profileSha256=expected_profile)
    command(["sudo", "-n", "apt-get", "install", "--yes", "--no-install-recommends",
             str(package)], timeout=600)
    for path in (EXECUTABLE,):
        root_owned_file(path)
    if package_profile == "package-profile-absent":
        require(not PROFILE.exists() and not PROFILE.is_symlink(),
                "package unexpectedly installed an AppArmor profile")
        custom_profile = directory / "chatgpt-apparmor-profile"
        journal = write_journal(directory, journal, phase="profile-install-started", createdProfile=True)
        custom_profile.write_text(CUSTOM_PROFILE)
        command(["sudo", "-n", "install", "-o", "root", "-g", "root", "-m", "644",
                 str(custom_profile), str(PROFILE)])
        command(["sudo", "-n", "apparmor_parser", "-r", str(PROFILE)])
    root_owned_file(PROFILE)
    validate_profile()
    journal = write_journal(directory, journal, phase="installed", profileSha256=digest(PROFILE),
                            executableSha256=digest(EXECUTABLE))
    profile_loaded_after = profile_loaded()
    require(profile_loaded_after, "official AppArmor profile is not loaded")
    require(restriction_value() == "1", "namespace restriction changed")
    require(command(["dpkg-query", "--show", "--showformat=${Version}", "chatgpt"]) == VERSION,
            "installed package version changed")
    evidence = {"schemaVersion": 1, "kind": "chatgpt-official-sandbox-setup",
                "installation": "official-system-package", "packageVersion": VERSION,
                "packageSha256": PACKAGE_SHA256, "executableSha256": digest(EXECUTABLE),
                "profileSha256": digest(PROFILE), "packageProfile": package_profile,
                "profileLoaded": profile_loaded_after,
                "profileAttachmentObservation": profile_attachment_observation(),
                "userNamespaceRestrictionBefore": 1, "userNamespaceRestrictionAfter": 1}
    with (directory / "sandbox-setup.json").open("x") as output:
        json.dump(evidence, output, sort_keys=True)
        output.write("\n")


def cleanup(directory, checker_report):
    """Remove only resources created by this experiment after process stop."""
    evidence_path = directory / "sandbox-cleanup.json"
    outcome = "cleaned"
    profile_removed = False
    package_removed = False
    before = "unknown"
    try:
        journal = bounded_json(directory / "sandbox-journal.json")
        require(journal.get("kind") == "chatgpt-sandbox-journal"
                and journal.get("schemaVersion") == 1
                and journal.get("packageSha256") == PACKAGE_SHA256
                and journal.get("packageVersion") == VERSION,
                "sandbox journal identity mismatch")
        phase = journal.get("phase")
        require(phase in {"package-verified", "install-started", "profile-install-started", "installed", "launch-intent"},
                "sandbox journal phase is not cleanup-safe")
        launch_state = journal.get("launchState")
        require(launch_state in {"prelaunch", "launched"}, "sandbox launch state is unknown")
    except (OSError, ValueError, json.JSONDecodeError):
        with evidence_path.open("x") as output:
            json.dump({"schemaVersion": 1, "kind": "chatgpt-official-sandbox-cleanup",
                       "outcome": "cleanup-uncertain", "profileRemoved": False,
                       "packageRemoved": False, "restrictionBefore": "unknown",
                       "restrictionAfter": "unknown"}, output, sort_keys=True)
            output.write("\n")
        raise ValueError("cleanup failed") from None
    try:
        process_check = command(["pgrep", "-x", "ChatGPT"], accepted=(0, 1))
        require(not process_check, "ChatGPT process is still present")
        if launch_state == "launched":
            require(checker_report is not None, "checker cleanup evidence is required")
            report = bounded_json(checker_report)
            try:
                validate_checker_report(report)
            except (AttributeError, TypeError, ValueError):
                raise ValueError("checker cleanup is unconfirmed") from None
        require(phase == "package-verified" or EXECUTABLE.exists(),
                "installed executable state is unknown")
        if EXECUTABLE.exists():
            root_owned_file(EXECUTABLE)
            require(digest(EXECUTABLE) == journal.get("executableSha256"),
                    "installed executable identity changed")
        if PROFILE.exists():
            root_owned_file(PROFILE)
            require(digest(PROFILE) == journal.get("profileSha256"),
                    "installed profile identity changed")
        if phase != "package-verified":
            require(command(["dpkg-query", "--show", "--showformat=${Version}", "chatgpt"]) == VERSION,
                    "installed package version changed")
        before = restriction_value()
        require(before == "1", "namespace restriction changed before cleanup")
        if phase == "package-verified":
            package = directory / "chatgpt.deb"
            require(package.is_file() and not package.is_symlink(), "package identity changed")
            require(digest(package) == PACKAGE_SHA256, "package identity changed")
            package.unlink()
            package_removed = True
        else:
            journal = write_journal(directory, journal, phase="profile-unload-started")
            command(["sudo", "-n", "apparmor_parser", "-R", str(PROFILE)])
            profile_removed = True
            journal = write_journal(directory, journal, phase="purge-started")
            command(["sudo", "-n", "apt-get", "purge", "--yes", "chatgpt"], timeout=600)
            package_removed = True
            if journal.get("createdProfile"):
                require(PROFILE.exists() and validate_profile() == CUSTOM_PROFILE,
                        "created AppArmor profile identity changed")
                command(["sudo", "-n", "rm", "--", str(PROFILE)])
                require(not PROFILE.exists(), "created AppArmor profile remains")
        require(not PROFILE.exists() and not EXECUTABLE.exists(),
                "experiment resources remain after cleanup")
        require(restriction_value() == "1", "namespace restriction changed after cleanup")
    except (OSError, ValueError, subprocess.SubprocessError):
        outcome = "cleanup-failed"
    after = restriction_value() if RESTRICTION.exists() else "unknown"
    evidence = {"schemaVersion": 1, "kind": "chatgpt-official-sandbox-cleanup",
                "outcome": outcome, "profileRemoved": profile_removed,
                "packageRemoved": package_removed, "restrictionBefore": before,
                "restrictionAfter": after if after in ("0", "1") else "unknown"}
    with evidence_path.open("x") as output:
        json.dump(evidence, output, sort_keys=True)
        output.write("\n")
    require(outcome == "cleaned", "cleanup failed")


def mark_launch(directory):
    journal = bounded_json(directory / "sandbox-journal.json")
    require(journal.get("kind") == "chatgpt-sandbox-journal"
            and journal.get("schemaVersion") == 1
            and journal.get("phase") == "installed"
            and journal.get("launchState") == "prelaunch",
            "sandbox launch state is not prelaunch")
    write_journal(directory, journal, phase="launch-intent", launchState="launched")


def main():
    os.umask(0o077)
    try:
        require(len(sys.argv) in (2, 3, 4), "runner directory and optional operation required")
        directory = Path(sys.argv[1])
        if len(sys.argv) == 3:
            require(sys.argv[2] == "--mark-launch", "unknown runner operation")
            mark_launch(directory)
        elif len(sys.argv) == 4:
            require(sys.argv[2] == "--cleanup", "unknown runner operation")
            cleanup(directory, Path(sys.argv[3]))
        else:
            install(directory)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        # Package command output is deliberately not forwarded to public logs.
        reason = str(error) if isinstance(error, ValueError) else "runner installation failed"
        print(reason, file=sys.stderr)
        return 1
    print("Pinned official package and AppArmor profile verified; namespace restriction unchanged.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
