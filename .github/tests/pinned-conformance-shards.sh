#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporary_directory="$(mktemp -d)"
trap 'rm -rf "$temporary_directory"' EXIT
bin_directory="$temporary_directory/bin"
mkdir -p "$bin_directory"

cat >"$temporary_directory/installer" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >>"$PINNED_INSTALL_LOG"
EOF
cat >"$bin_directory/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s|%s\n' "${CLINE_NO_AUTO_UPDATE:-unset}" "$*" >>"$PINNED_CARGO_LOG"
EOF
chmod 755 "$temporary_directory/installer" "$bin_directory/cargo"

expected=(
  aider claude-code cline codex deepseek-harness fx goose hermes kimi-code
  omp openclaw opencode pi prime-agent qwen-code
)
configured="$(
  sed -n 's/^[[:space:]]*harnesses: //p' "$repository_root/.github/workflows/pinned-conformance.yml" \
    | tr ' ' '\n' \
    | sort \
    | tr '\n' ' '
)"
[ "$configured" = "${expected[*]} " ] || {
  printf 'pinned conformance shards do not cover every harness exactly once\n' >&2
  exit 1
}

PINNED_INSTALL_LOG="$temporary_directory/install.log" \
PINNED_CARGO_LOG="$temporary_directory/cargo.log" \
NAN_PINNED_INSTALLER="$temporary_directory/installer" \
PATH="$bin_directory:$PATH" \
  bash "$repository_root/.github/scripts/run-pinned-conformance.sh" "${expected[@]}"

diff -u <(printf '%s\n' "${expected[@]}") "$temporary_directory/install.log"
grep -Fq 'conformance_claude claude_code_tools_complete_their_conformance_scenarios' "$temporary_directory/cargo.log"
grep -Fq 'conformance_codex codex_native_inventory_crosses_the_responses_bridge' "$temporary_directory/cargo.log"
grep -Fq 'conformance_fx fx_' "$temporary_directory/cargo.log"
grep -Fq 'conformance_direct deepseek_harness_' "$temporary_directory/cargo.log"
grep -Fq '1|run --quiet -- doctor cline' "$temporary_directory/cargo.log"
grep -Fq '1|test -p nan-harness-cli --test conformance_direct cline_' "$temporary_directory/cargo.log"
grep -Fq 'unset|run --quiet -- doctor qwen' "$temporary_directory/cargo.log"
