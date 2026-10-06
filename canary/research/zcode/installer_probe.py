"""Exercise the unmodified upstream Unix installer using a locally built package."""

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import subprocess
import tempfile
import threading

from native_probe import Scenario, exercise_native, make_handler


class QuietFiles(SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--node", required=True)
    args = parser.parse_args()
    source = args.source.resolve()
    distribution = source / "dist/zcode"
    files = ThreadingHTTPServer(("127.0.0.1", 0), partial(QuietFiles, directory=str(distribution)))
    scenario = Scenario()
    provider = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(scenario))
    threads = [
        threading.Thread(target=server.serve_forever, daemon=True) for server in (files, provider)
    ]
    for thread in threads:
        thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="nanh-zcode-install-") as temporary:
            home = Path(temporary)
            settings = home / ".zcode/cli/config.json"
            settings.parent.mkdir(parents=True)
            original = '{"locale":"en-US","features":{"mcp":true}}\n'
            settings.write_text(original)
            env = {
                "HOME": temporary,
                "PATH": str(Path(args.node).parent) + os.pathsep + os.environ["PATH"],
                "ZCODE_DIST_BASE_URL": f"http://127.0.0.1:{files.server_port}",
            }
            for custom in (False, True):
                if custom:
                    env.update(
                        ZCODE_DIST_HOME=str(home / "custom-runtime"),
                        ZCODE_DIST_BIN_DIR=str(home / "custom-bin"),
                    )
                binary = Path(env.get("ZCODE_DIST_BIN_DIR", str(home / ".local/bin"))) / "zcode"
                for _ in range(2):
                    installed = subprocess.run(
                        ["sh", str(distribution / "install.sh")],
                        env=env,
                        capture_output=True,
                        text=True,
                        timeout=120,
                    )
                    assert installed.returncode == 0, "Official installer failed"
                    assert (
                        settings.read_text() == original
                    ), "Installer changed existing user settings"
                    product = subprocess.run(
                        [str(binary), "--version"],
                        env=env,
                        capture_output=True,
                        text=True,
                        timeout=15,
                    )
                    agent = subprocess.run(
                        [str(binary), "version"],
                        env=env,
                        capture_output=True,
                        text=True,
                        timeout=15,
                    )
                    assert product.returncode == agent.returncode == 0
                    assert product.stdout.strip() == "3.14.3" and agent.stdout.strip() == "0.16.9"
                print(
                    "PASS: official installer, repeat install, preserved settings, version surfaces, "
                    + ("custom paths" if custom else "default paths")
                )
            runtime = home / "custom-runtime/current/bin/zcode.mjs"
            exercise_native(source, args.node, scenario, provider.server_port, runtime)
    finally:
        for server in (files, provider):
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=5)


if __name__ == "__main__":
    main()
