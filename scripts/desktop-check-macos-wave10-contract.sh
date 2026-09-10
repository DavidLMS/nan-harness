#!/usr/bin/env bash
# Temporary pre-push contract for the wave10 Zed macOS occlusion diagnostic.
# Owned only by this experimental task; remove once the run concludes.
#
# It verifies the temporary workflow's static shell contracts before any push:
#   1. the YAML parses under a safe parser,
#   2. every `run:` shell block is syntactically valid Bash,
#   3. a failing command propagates a nonzero exit through a pipe under
#      `set -euo pipefail`,
#   4. a successful command under the same pipeline exits zero,
#   5. the tracked/untracked owned files pass whitespace checks.
set -euo pipefail

workflow=".github/workflows/desktop-check-macos-wave10.yml"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "== 1. YAML parse (safe Psych) =="
ruby -ryaml -e 'YAML.load_file(ARGV.fetch(0)); puts "yaml-ok"' "$workflow"

echo "== 2. Bash syntax of every run: block =="
ruby -ryaml -rjson -e '
  doc = YAML.load_file(ARGV.fetch(0))
  blocks = []
  jobs = doc.fetch("jobs")
  jobs.each do |job_name, job|
    job.fetch("steps").each_with_index do |step, idx|
      if step.is_a?(Hash) && step["run"].is_a?(String)
        blocks << { name: step["name"] || "#{job_name} step #{idx}", script: step["run"] }
      end
    end
  end
  blocks.each_with_index do |b, i|
    File.write(File.join(ARGV.fetch(1), "run_#{i}.sh"), b[:script])
    warn "run_#{i}.sh  <- #{b[:name]}"
  end
  puts "blocks=#{blocks.length}"
' "$workflow" "$tmpdir"
count=0
for f in "$tmpdir"/run_*.sh; do
  bash -n "$f"
  count=$((count+1))
done
echo "bash-syntax-ok blocks=$count"

echo "== 3. Nonzero pipeline propagation (Cargo is trusted) =="
set -o pipefail
if (set -e; false | tee "$tmpdir/tee.out") 2>/dev/null; then
  echo "pipefail did NOT propagate a failing command; contract broken" >&2
  exit 1
fi
echo "pipefail-propagates=ok"

echo "== 4. Successful command pipeline exits zero =="
if ! (set -e; true | tee "$tmpdir/tee-ok.out") >/dev/null; then
  echo "a successful pipeline reported failure; contract broken" >&2
  exit 1
fi
echo "pipe-success=ok"

echo "== 5. Whitespace checks on the owned temp files =="
bad=""
for f in "$workflow" scripts/desktop-check-macos-wave10-contract.sh; do
  if grep -n '[[:blank:]]$' "$f" >/dev/null; then
    echo "trailing whitespace in $f" >&2; bad=1
  fi
  if grep -n "$(printf '\t')" "$f" >/dev/null; then
    echo "tab character in $f" >&2; bad=1
  fi
  [ -n "$(tail -c1 "$f")" ] && { echo "$f lacks a trailing newline" >&2; bad=1; }
done
[ -n "$bad" ] && { echo "whitespace contract broken" >&2; exit 1; }
echo "whitespace-ok"

echo "ALL WAVE10 PRE-PUSH CONTRACTS PASSED"
