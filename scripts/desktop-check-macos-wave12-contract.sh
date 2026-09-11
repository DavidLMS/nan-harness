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
    scripts/wave12-env/tests/test_staging.py \
    scripts/wave12-env/tests/run-synthetic-tests.sh \
    scripts/wave12-env/fixtures/bin/*; do
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

echo "== 8. Static fake-backend isolation (no route to a host command) =="
ruby -e '
  dir = "scripts/wave12-env"
  absolute_tool = /(?<![\w.}\-])\/(usr\/)?s?bin\//
  src = File.read("#{dir}/run-experiment.sh")
  # The quarantined real backend must execute nothing.
  body = src[/^real_driver\(\) \{\n(.*?)^\}\n/m, 1] or abort "real_driver is missing"
  body.each_line do |line|
    s = line.strip
    next if s.empty? || s.start_with?("#")
    unless s == "return 78" || s.match?(/\Aecho \x27[^\x27]*\x27 >&2\z/)
      abort "real_driver executes something: #{s}"
    end
  end
  # Without an explicit fake backend the script must refuse before anything else.
  refusal = src[/^  if \[ -z "\$FAKE_BACKEND" \]; then\n(.*?)^  fi\n/m, 1] or abort "quarantine refusal is missing"
  code = refusal.lines.map(&:strip).reject { |s| s.empty? || s.start_with?("#") }
  abort "quarantine must only report and exit 78" unless code.size == 2 && code.last == "exit 78"
  abort "backend must be selected before main" unless src.index("\nselect_backend\n") < src.index("\nmain \"$@\"")
  # Host command names may only appear as driver arguments or allowlists.
  host = /\b(defaults|killall|pkill|pgrep|uname|sw_vers|osascript|launchctl|screencapture)\b/
  src.each_line.with_index(1) do |line, n|
    # The marker comparison names the shebang as data, not as a tool path.
    s = line.strip.sub("\"#!/usr/bin/env bash\"", "\"shebang\"")
    next if n == 1 || s.start_with?("#")
    abort "absolute tool path at run-experiment.sh:#{n}" if s.match?(absolute_tool)
    next unless s.match?(host)
    next if s.match?(/\bdriver\b/) || s.match?(/\A(HOST_COMMANDS|FAKE_DRIVERS)=/)
    next if s.start_with?("defaults|killall|pgrep|uname|runner-environment)")
    abort "host command outside a driver call at run-experiment.sh:#{n}: #{s}"
  end
  # Every fake driver carries the marker and resolves nothing by absolute path.
  drivers = Dir.glob("#{dir}/fixtures/bin/*").sort
  abort "fake drivers missing" if drivers.empty?
  drivers.each do |file|
    lines = File.readlines(file)
    unless lines[0] == "#!/usr/bin/env bash\n" && lines[1] == "# wave12-fake-driver\n"
      abort "#{file} lacks the fake-driver marker"
    end
    lines.each_with_index.drop(1).each do |line, i|
      abort "absolute tool path at #{file}:#{i + 1}" if line.match?(absolute_tool)
    end
  end
  # Every orchestrator or staging process in the suite starts from env -i with
  # the toolbox PATH.
  harness = File.readlines("#{dir}/tests/run-synthetic-tests.sh")
  launches = 0
  harness.each_with_index do |line, i|
    if line.include?("\"$orchestrator\"") || line.include?("stage-artifacts.sh\"")
      abort "unisolated launch at run-synthetic-tests.sh:#{i + 1}" unless line.include?("\"$toolbox/bash\"")
    end
    next unless line.include?("\"$toolbox/bash\"")
    j = i
    j -= 1 while j > 0 && harness[j - 1].rstrip.end_with?("\\")
    unless harness[j].lstrip.start_with?("env -i PATH=\"$toolbox\" ")
      abort "launch without env -i toolbox PATH at run-synthetic-tests.sh:#{i + 1}"
    end
    launches += 1
  end
  abort "no isolated launches found" if launches.zero?
  puts "fake-isolation=ok launches=#{launches}"
'

echo "== 9. Workflow keeps real drivers quarantined and never spoofs the runner =="
ruby -ryaml -e '
  path = ARGV.fetch(0)
  doc = YAML.load_file(path)
  text = File.read(path)
  jobs = doc.fetch("jobs").values
  steps = jobs.flat_map { |j| j.fetch("steps") }
  envs = [doc["env"], *jobs.map { |j| j["env"] }, *steps.map { |s| s["env"] }].compact
  envs.each do |env|
    %w[GITHUB_ACTIONS RUNNER_ENVIRONMENT].each do |key|
      abort "workflow overrides runner signal #{key}" if env.key?(key)
    end
  end
  abort "workflow enables or bypasses real drivers" if text.match?(/WAVE12_REAL_DRIVERS|--fake-backend|--fixture-bin/)
  suite = steps.index { |s| s["run"].to_s.include?("scripts/wave12-env/tests/run-synthetic-tests.sh") }
  first = steps.index { |s| s["run"].to_s.include?("scripts/wave12-env/run-experiment.sh") }
  abort "fake-backend suite step missing" unless suite
  abort "condition step missing" unless first
  abort "fake-backend suite must gate every condition" if suite > first || steps[suite]["continue-on-error"]
  gate = steps.find { |s| s["id"] == "conditions-gate" } or abort "conditions gate missing"
  %w[baseline dock-hidden].each do |id|
    abort "missing condition step id #{id}" unless steps.any? { |s| s["id"] == id }
    abort "conditions gate ignores #{id}" unless gate.to_s.include?("steps.#{id}.outcome")
  end
  abort "conditions gate must be last" unless steps.last.equal?(gate)
  puts "workflow-quarantine=ok"
' "$workflow"

echo "ALL WAVE12 PRE-PUSH CONTRACTS PASSED"
