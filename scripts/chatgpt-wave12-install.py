#!/usr/bin/env python3
"""Install the pinned official package only on a disposable GitHub Linux runner.

The private checker installer intentionally does not run package maintainer
scripts. This experiment instead uses the publisher's system installation and
exact AppArmor attachment path, without relaxing the host namespace policy.
"""

import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys


VERSION = "26.908.40834"
PACKAGE_SHA256 = "da37b8e7bcefaaea019c478cacbe6c73ee1ddd15e0e1ebb3c7ef0a42dd818ac2"
PACKAGE_URL = ("https://persistent.oaistatic.com/codex-app-prod/linux/deb/"
               f"pool/main/c/chatgpt/chatgpt_{VERSION}_amd64.deb")
EXECUTABLE = Path("/usr/lib/chatgpt/ChatGPT")
PROFILE = Path("/etc/apparmor.d/chatgpt")
RESTRICTION = Path("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
OFFICIAL_PROFILE = ('abi <abi/4.0>,\ninclude <tunables/global>\n\n'
                    'profile chatgpt "/usr/lib/chatgpt/ChatGPT" flags=(unconfined) {\n'
                    '  userns,\n  include if exists <local/chatgpt>\n}\n')


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def command(arguments, timeout=30, accepted=(0,)):
    result = subprocess.run(arguments, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=timeout, check=False, text=True)
    require(result.returncode in accepted, "runner command failed")
    return result.stdout.strip()


def profile_loaded():
    profiles = command(["sudo", "-n", "cat", "/sys/kernel/security/apparmor/profiles"])
    return any(line in ("chatgpt (unconfined)", "chatgpt (enforce)")
               for line in profiles.splitlines())


def check_runner():
    expected = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted",
                "RUNNER_OS": "Linux", "RUNNER_ARCH": "X64"}
    require(all(os.environ.get(key) == value for key, value in expected.items())
            and platform.system() == "Linux" and platform.machine() == "x86_64"
            and os.getuid() != 0, "disposable Linux x64 runner required")
    require(not any(key in os.environ for key in
                    ("NAN_API_KEY", "OPENAI_API_KEY", "CODEX_API_KEY")),
            "remove provider credentials before installation")
    require(RESTRICTION.read_text().strip() == "1", "namespace restriction must remain enabled")
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
    package = directory / "chatgpt.deb"
    command(["curl", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https",
             "--max-time", "300", "--max-filesize", "450000000", "--silent", "--show-error",
             "--output", str(package), PACKAGE_URL], timeout=310)
    require(digest(package) == PACKAGE_SHA256, "official package digest mismatch")
    for field, expected in (("Package", "chatgpt"), ("Version", VERSION), ("Architecture", "amd64")):
        require(command(["dpkg-deb", "--field", str(package), field]) == expected,
                "official package identity mismatch")
    command(["sudo", "-n", "apt-get", "install", "--yes", "--no-install-recommends",
             str(package)], timeout=600)
    for path in (EXECUTABLE, PROFILE):
        root_owned_file(path)
    require(PROFILE.read_text() == OFFICIAL_PROFILE, "official AppArmor profile changed")
    require(profile_loaded(), "official AppArmor profile is not loaded")
    require(RESTRICTION.read_text().strip() == "1", "namespace restriction changed")
    require(command(["dpkg-query", "--show", "--showformat=${Version}", "chatgpt"]) == VERSION,
            "installed package version changed")
    evidence = {"schemaVersion": 1, "kind": "chatgpt-official-sandbox-setup",
                "installation": "official-system-package", "packageVersion": VERSION,
                "packageSha256": PACKAGE_SHA256, "executableSha256": digest(EXECUTABLE),
                "profileSha256": digest(PROFILE), "profileLoaded": True,
                "userNamespaceRestrictionBefore": 1, "userNamespaceRestrictionAfter": 1}
    with (directory / "sandbox-setup.json").open("x") as output:
        json.dump(evidence, output, sort_keys=True)
        output.write("\n")


def main():
    os.umask(0o077)
    try:
        require(len(sys.argv) == 2, "one new runner directory is required")
        install(Path(sys.argv[1]))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        # Package command output is deliberately not forwarded to public logs.
        reason = str(error) if isinstance(error, ValueError) else "runner installation failed"
        print(reason, file=sys.stderr)
        return 1
    print("Pinned official package and AppArmor profile verified; namespace restriction unchanged.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
