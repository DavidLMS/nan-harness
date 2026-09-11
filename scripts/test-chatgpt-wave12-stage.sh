#!/usr/bin/env bash
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
tmp=$(mktemp -d /tmp/chatgpt-wave12-stage.XXXXXX)
chmod 700 "$tmp"
trap 'rm -rf -- "$tmp"' EXIT
facts="$tmp/facts.json"; report="$tmp/report.json"; wrapper="$tmp/wrapper.sh"
reducer="$root/chatgpt-wave12-reducer.py"; checker="$tmp/checker"
python3 - "$facts" "$reducer" <<'PY'
import importlib.util
import json
import sys
spec = importlib.util.spec_from_file_location("r", sys.argv[2])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
bounds = {"maxStreamBytes": 1048576, "maxLineBytes": 8192, "maxLines": 65535,
          "deadlineSeconds": 60, "graceSeconds": 5}
facts = module.refusal_facts("runtime-refused", bounds, None)
facts.update(failure="none", observation="complete", classification="no-signature",
             launcherDisposition="exited", launcherExit=0)
facts["identity"] = {key: "0" * 64 for key in ("realNanhSha256", "shimSha256", "reducerSha256")}
assert module.validate_facts(facts) is None
with open(sys.argv[1], "w", encoding="utf-8") as output:
    json.dump(facts, output)
PY
printf '{"ok":true}\n' > "$report"
printf '#!/usr/bin/env bash\nexit 0\n' > "$wrapper"; chmod 700 "$wrapper"
printf '#!/usr/bin/env bash\nset -eu\nsha256sum "$2" | cut -d" " -f1\n' > "$checker"; chmod 700 "$checker"
destination="$tmp/envelope.json"
bash "$root/chatgpt-wave12-stage.sh" "$report" "$facts" "$wrapper" "$reducer" "$checker" "$destination"
jq -e '.diagnosticVersion == 1 and .kind == "chatgpt-startup-wrapper" and (.observation | type == "object") and (.report.ok == true)' "$destination" >/dev/null
test "$(stat -c '%a' "$destination" 2>/dev/null || stat -f '%Lp' "$destination")" = 600
printf 'wave12 staging contract ok\n'
