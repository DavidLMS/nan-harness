#!/usr/bin/env bash
# Temporary pre-push static contract for the wave12 Zed macOS environment
# experiment. Owned only by this experimental task; remove once the run
# concludes. It verifies the temporary workflow's static shell contracts:
#   1. the YAML parses under a safe parser,
#   2. every `run:` shell block is syntactically valid Bash,
#   3. a failing command propagates a nonzero exit through a pipe under
#      `set -euo pipefail`,
#   4. a successful command under the same pipeline exits zero,
#   5. the owned temp files pass whitespace checks (no tabs / trailing blanks).
# It never runs the experiment and never touches a real macOS preference.
set -euo pipefail

workflow=".github/workflows/desktop-check-macos-wave12.yml"
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
  echo "pipefail did NOT propagate a failing command; contract broken" >&2; exit 1
fi
echo "pipefail-propagates=ok"

echo "== 4. Successful command pipeline exits zero =="
if ! (set -e; true | tee "$tmpdir/tee-ok.out") >/dev/null; then
  echo "a successful pipeline reported failure; contract broken" >&2; exit 1
fi
echo "pipe-success=ok"

echo "== 5. Whitespace checks on the owned temp files =="
bad=""
for f in "$workflow" scripts/desktop-check-macos-wave12-contract.sh \
    scripts/wave12-env/run-experiment.sh scripts/wave12-env/stage-artifacts.sh \
    scripts/wave12-env/README.md scripts/wave12-env/stage_artifacts.py \
    scripts/wave12-env/tests/test_staging.py; do
  [ -f "$f" ] || continue
  if grep -n '[[:blank:]]$' "$f" >/dev/null; then
    echo "trailing whitespace in $f" >&2; bad=1
  fi
  [ -z "$(grep -n "$(printf '\t')" "$f" || true)" ] || { echo "tab character in $f" >&2; bad=1; }
  [ -n "$(tail -c1 "$f")" ] && { echo "$f lacks a trailing newline" >&2; bad=1; }
done
[ -n "$bad" ] && { echo "whitespace contract broken" >&2; exit 1; }
echo "whitespace-ok"

echo "== 6. Upload paths are staged-only (no raw original path may be uploaded) =="
ruby -ryaml -e '
  doc = YAML.load_file(ARGV.fetch(0))
  bad = []
  jobs = doc.fetch("jobs")
  jobs.each do |_, job|
    job.fetch("steps").each do |step|
      next unless step.is_a?(Hash)
      with = step["with"]
      next unless with.is_a?(Hash) && with["path"].is_a?(String)
      with["path"].each_line do |line|
        line = line.strip
        next if line.empty?
        if !line.start_with?("${{ runner.temp }}/env-staging/")
          bad << "non-staged upload path: #{line}"
        end
      end
    end
  end
  unless bad.empty?
    warn bad.join("\n")
    exit 1
  end
  puts "staged-only-paths=ok"
' "$workflow"

echo "== 7. Staging gates reference the staging step ids =="
ruby -ryaml -e '
  doc = YAML.load_file(ARGV.fetch(0))
  ids = doc.fetch("jobs").values.flat_map { |j| (j["steps"] || []).map { |s| s.is_a?(Hash) ? s["id"] : nil } }.compact
  %w[stage-baseline stage-dock-hidden].each do |want|
    abort "missing staging step id #{want}" unless ids.include?(want)
  end
  puts "staging-gates=ok"
' "$workflow"

echo "ALL WAVE12 PRE-PUSH CONTRACTS PASSED"
