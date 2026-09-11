#!/usr/bin/env bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
tmp=$(mktemp -d /tmp/chatgpt-wave12-stage.XXXXXX)
chmod 700 "$tmp"
trap 'rm -rf -- "$tmp"' EXIT
diagnostic="$tmp/diagnostic.json"; facts="$tmp/facts.json"
wrapper="$tmp/wrapper.sh"; nanh="$tmp/nanh"; destination="$tmp/staged.json"
reducer="$root/chatgpt-wave12-reducer.py"
checker="${CHECKER:-$root/../target/debug/nanh-desktop-check}"
test -x "$checker" || exit 1
printf '#!/usr/bin/env bash\nexit 0\n' > "$wrapper"; chmod 700 "$wrapper"
printf '#!/usr/bin/env bash\nexit 0\n' > "$nanh"; chmod 700 "$nanh"
python3 - "$diagnostic" "$facts" "$wrapper" "$reducer" "$nanh" <<'PY'
import hashlib, importlib.util, json, sys
from pathlib import Path
diagnostic, facts_path, wrapper, reducer_path, nanh = map(Path, sys.argv[1:])
spec = importlib.util.spec_from_file_location("reducer", reducer_path)
reducer = importlib.util.module_from_spec(spec); spec.loader.exec_module(reducer)
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
bounds = {"maxStreamBytes": 1048576, "maxLineBytes": 8192, "maxLines": 65535, "deadlineSeconds": 60, "graceSeconds": 5}
facts = reducer.refusal_facts("runtime-refused", bounds, None)
facts["identity"] = {"realNanhSha256": sha(nanh), "shimSha256": sha(wrapper), "reducerSha256": sha(reducer_path)}
assert reducer.validate_facts(facts) is None
facts_path.write_text(json.dumps(facts), encoding="utf-8")
report = {"schemaVersion": 2, "checkerVersion": "0.1.4", "runId": "22222222222222222222222222222222", "startedAt": "2026-09-10T12:00:00Z", "platform": "linux", "architecture": "x86_64", "nanHarness": None, "results": [{"app": "chatgpt-desktop", "appVersion": None, "runtimeVersion": None, "deterministic": [{"status": "blocked", "reason": "not-run", "steps": [], "durationMilliseconds": 0} for _ in range(3)], "live": {"status": "skipped", "reason": "missing-key", "steps": [], "durationMilliseconds": 0}, "cleanup": "passed"}], "cleanup": "passed"}
diagnostic.write_text(json.dumps({"diagnosticVersion": 1, "kind": "chatgpt-startup-wrapper", "wrapperSha256": sha(wrapper), "observation": report}, indent=2), encoding="utf-8")
PY
stage() { bash "$root/chatgpt-wave12-stage.sh" "$@"; }
stage "$diagnostic" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$destination"
python3 - "$destination" "$wrapper" <<'PY'
import hashlib, json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
assert set(value) == {"diagnosticVersion", "kind", "wrapperSha256", "observation"}
assert value["wrapperSha256"] == hashlib.sha256(open(sys.argv[2], "rb").read()).hexdigest()
PY
test "$(stat -c '%a' "$destination" 2>/dev/null || stat -f '%Lp' "$destination")" = 600
expect_refused() { if stage "$@"; then exit 1; fi; }
expect_refused "$diagnostic" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$destination"
python3 - "$facts" <<'PY'
import json, sys
path = sys.argv[1]; value = json.load(open(path)); value["identity"]["reducerSha256"] = "0" * 64; json.dump(value, open(path, "w"))
PY
expect_refused "$diagnostic" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$tmp/facts-identity.json"
python3 - "$diagnostic" <<'PY'
import json, sys
path = sys.argv[1]; value = json.load(open(path)); value["privateMarker"] = True; json.dump(value, open(path, "w"))
PY
expect_refused "$diagnostic" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$tmp/private.json"
python3 - "$diagnostic" <<'PY'
import json, sys
path = sys.argv[1]; value = json.load(open(path)); value["wrapperSha256"] = "0" * 64; json.dump(value, open(path, "w"))
PY
expect_refused "$diagnostic" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$tmp/identity.json"
ln -s "$diagnostic" "$tmp/diagnostic-link.json"
expect_refused "$tmp/diagnostic-link.json" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$tmp/link.json"
printf '{}' > "$tmp/overwrite.json"
expect_refused "$diagnostic" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$tmp/overwrite.json"
python3 - "$tmp/large.json" <<'PY'
import sys
with open(sys.argv[1], "w") as out: out.write("{" + "x" * (8 * 1024 * 1024) + "}")
PY
expect_refused "$tmp/large.json" "$facts" "$wrapper" "$reducer" "$checker" "$nanh" "$tmp/large-out.json"
printf 'wave12 staging contract ok\n'
