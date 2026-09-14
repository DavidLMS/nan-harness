[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][ValidateSet('claude-code','codex','opencode','hermes','pi','omp','prime-agent','deepseek-harness','openclaw','cline','qwen-code','kimi-code','aider','goose','fx')][string]$Harness,
  [Parameter(Mandatory=$true)][ValidateSet('version-doctor','deterministic-contract','live-tool')][string]$Stage,
  [string]$Model = 'qwen3.6', [Parameter(Mandatory=$true)][string]$NanBinary,
  [Parameter(Mandatory=$true)][string]$Canary, [Parameter(Mandatory=$true)][string]$Version
)
$ErrorActionPreference = 'Stop'; $stageNow = $Stage; $workspace = $null; $stdout = $null; $stderr = $null; $completed = $false; $markerPath = $env:NAN_CANARY_PROBE_RESULT; $diagnostics = New-Object System.Collections.Generic.List[string]; $exitCode = $null
$knownDiagnostics = @('doctor-child-launch','doctor-exit-nonzero','doctor-output-invalid','doctor-schema-invalid','doctor-version-mismatch','doctor-exit-missing','conformance-child-launch','conformance-exit-nonzero','conformance-output-invalid','conformance-schema-invalid','conformance-scenario-missing','conformance-scenario-failed','conformance-inventory-failed','conformance-check-invalid','conformance-exit-missing','live-child-launch','live-exit-nonzero','live-exit-missing','live-credential-missing','live-tool-evidence-missing','live-read-marker-missing','live-completion-marker-missing','live-bridge-sentinel','live-usage-invalid','live-usage-summary-missing')
function Add-Diagnostic([string]$Code) { if ($knownDiagnostics -contains $Code -and -not $diagnostics.Contains($Code)) { [void]$diagnostics.Add($Code) } }
function Write-Result([string]$resultStage, [string]$status) {
  if (-not $markerPath) { return }; $parent = Split-Path -Parent $markerPath
  $value = [ordered]@{ schemaVersion = 2; stage = $resultStage; status = $status; diagnostics = @($diagnostics.ToArray()) }
  if ($null -ne $exitCode) { $value.exitCode = [int]$exitCode }
  $tmp = Join-Path $parent ('.probe-result.' + [guid]::NewGuid().ToString('N'))
  # Windows PowerShell 5.1 supports UTF-8 (with BOM); JSON parsing is encoding-aware.
  try { $value | ConvertTo-Json -Compress | Set-Content -LiteralPath $tmp -Encoding UTF8; Move-Item -LiteralPath $tmp -Destination $markerPath -Force }
  finally { if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue } }
}
function Fail([string]$message) {
  $code = switch ($stageNow) {
    'live-tool' { 'live-credential-missing' }
    'harness-run' { 'live-tool-evidence-missing' }
    'read-marker' { 'live-read-marker-missing' }
    'completion-marker' { 'live-completion-marker-missing' }
    'bridge-sentinel' { 'live-bridge-sentinel' }
    'usage-evidence' { 'live-usage-invalid' }
    'usage-summary' { 'live-usage-summary-missing' }
    default { $null }
  }
  if ($code) { Add-Diagnostic $code }; throw $message
}
function Run-Native([string[]]$Arguments) {
  try { & $NanBinary @Arguments 1> $stdout 2> $stderr; $exitCode = $LASTEXITCODE }
  catch { Add-Diagnostic 'live-child-launch'; $exitCode = -1; throw }
  if ($exitCode -ne 0) { Add-Diagnostic 'live-exit-nonzero'; throw 'harness command failed' }
}
function Has-Text([string]$Pattern) { return [bool](Select-String -LiteralPath @($stdout, $stderr) -Pattern $Pattern -SimpleMatch -Quiet -ErrorAction SilentlyContinue) }
function Has-Regex([string]$Pattern) { return [bool](Select-String -LiteralPath @($stdout, $stderr) -Pattern $Pattern -Quiet -ErrorAction SilentlyContinue) }
function Read-Exact([string]$Path, [string]$Expected) { if (-not (Test-Path -LiteralPath $Path)) { Fail 'tool result missing' }; if ((Get-Content -Raw -LiteralPath $Path).TrimEnd("`r", "`n") -cne $Expected) { Fail 'tool result mismatch' } }
function Is-NonNegativeInteger($Value) { return ($Value -is [int] -or $Value -is [long] -or $Value -is [int32] -or $Value -is [int64]) -and [int64]$Value -ge 0 }
function Has-OnlyProperties([object]$Value, [string[]]$Allowed) { return -not (@($Value.PSObject.Properties.Name | Where-Object { $Allowed -notcontains $_ }).Count -gt 0) }
function Validate-Conformance([object]$Value, [string]$ExpectedHarness) {
  $valid = $true; $names = @('external-prerequisite','inventory','sentinel','tool-round-trip'); $seen = @{}
  if ($null -eq $Value -or $Value.schemaVersion -notin @(1,2) -or [string]$Value.harness -cne $ExpectedHarness -or [string]$Value.outcome -notin @('passed','failed') -or $null -eq $Value.scenarios -or -not (Has-OnlyProperties $Value @('schemaVersion','harness','scenarios','outcome','durationMilliseconds','observations')) -or -not (Is-NonNegativeInteger $Value.durationMilliseconds) -or [int64]$Value.durationMilliseconds -gt 86400000) { Add-Diagnostic 'conformance-schema-invalid'; return $false }
  if ($Value.schemaVersion -eq 1 -and $null -ne $Value.observations -and @($Value.observations).Count -gt 0) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  if ($Value.schemaVersion -eq 2 -and $null -ne $Value.observations) {
    $observations = @($Value.observations); if ($observations.Count -gt 1) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
    foreach ($observation in $observations) { if (-not (Has-OnlyProperties $observation @('kind','fingerprint')) -or [string]$observation.kind -cne 'inventory-drift' -or [string]$observation.fingerprint -notmatch '^[0-9a-fA-F]{64}$') { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false } }
  }
  $scenarios = @($Value.scenarios); if ($scenarios.Count -ne 4) { Add-Diagnostic 'conformance-scenario-missing'; $valid = $false }
  foreach ($scenario in $scenarios) {
    $name = [string]$scenario.name; if (-not (Has-OnlyProperties $scenario @('name','status','checks','durationMilliseconds')) -or $name.Length -gt 64 -or $names -notcontains $name -or $seen.ContainsKey($name)) { Add-Diagnostic 'conformance-scenario-missing'; $valid = $false } else { $seen[$name] = $true }
    $allowed = if ($name -eq 'external-prerequisite') { @('passed','skipped','failed') } else { @('passed','failed') }
    if ($allowed -notcontains [string]$scenario.status -or $null -eq $scenario.checks -or @($scenario.checks).Count -lt 1 -or @($scenario.checks).Count -gt 8 -or -not (Is-NonNegativeInteger $scenario.durationMilliseconds) -or [int64]$scenario.durationMilliseconds -gt 86400000) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
    if ([string]$scenario.status -eq 'failed') { Add-Diagnostic 'conformance-scenario-failed'; if ($name -eq 'inventory') { Add-Diagnostic 'conformance-inventory-failed' } }
    foreach ($check in @($scenario.checks)) { if (-not (Has-OnlyProperties $check @('name','status','durationMilliseconds')) -or [string]::IsNullOrEmpty([string]$check.name) -or ([string]$check.name).Length -gt 64 -or [string]$check.status -notin @('passed','failed','skipped') -or -not (Is-NonNegativeInteger $check.durationMilliseconds) -or [int64]$check.durationMilliseconds -gt 86400000) { Add-Diagnostic 'conformance-check-invalid'; $valid = $false } }
  }
  if ($Value.outcome -eq 'passed' -and $diagnostics.Contains('conformance-scenario-failed')) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  if ($Value.outcome -eq 'failed' -and -not $diagnostics.Contains('conformance-scenario-failed')) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  return $valid
}
try {
  $workspace = Join-Path ([System.IO.Path]::GetTempPath()) ('nan-canary-' + [guid]::NewGuid().ToString('N')); New-Item -ItemType Directory -Path $workspace -Force | Out-Null
  $stdout = Join-Path $workspace 'harness-output.txt'; $stderr = Join-Path $workspace 'harness-stderr.txt'
  if ($Stage -eq 'version-doctor') {
    try { & $NanBinary 'doctor' $Harness '--allow-unsupported' '--allow-untested' '--json' 1> $stdout 2> $stderr; $exitCode = $LASTEXITCODE } catch { Add-Diagnostic 'doctor-child-launch'; $exitCode = -1 }
    if ($exitCode -ne 0) { Add-Diagnostic 'doctor-exit-nonzero' }
    try { $value = Get-Content -Raw $stdout | ConvertFrom-Json } catch { Add-Diagnostic 'doctor-output-invalid'; $value = $null }
    if ($null -ne $value -and ([int]$value.schemaVersion -ne 8 -or [string]$value.harness -cne $Harness -or [string]$value.level -notin @('ok','warning','info','error') -or -not ($value.safeToShare -is [bool]))) { Add-Diagnostic 'doctor-schema-invalid' }
    if ($null -ne $value -and (-not $value.version -or [string]$value.version -cne $Version)) { Add-Diagnostic 'doctor-version-mismatch' }
    if ($diagnostics.Count -eq 0 -and $null -eq $exitCode) { Add-Diagnostic 'doctor-exit-missing' }
    if ($diagnostics.Count -eq 0) { $completed = $true }; return
  }
  if ($Stage -eq 'deterministic-contract') {
    try { & $Canary 'conformance' '--nan-harness' $NanBinary '--harness' $Harness '--json' 1> $stdout 2> $stderr; $exitCode = $LASTEXITCODE } catch { Add-Diagnostic 'conformance-child-launch'; $exitCode = -1 }
    if ($exitCode -ne 0) { Add-Diagnostic 'conformance-exit-nonzero' }
    try { $value = Get-Content -Raw $stdout | ConvertFrom-Json } catch { Add-Diagnostic 'conformance-output-invalid'; $value = $null }
    if ($null -eq $value) { Add-Diagnostic 'conformance-schema-invalid' } else { [void](Validate-Conformance $value $Harness) }
    if ($diagnostics.Count -eq 0 -and $null -eq $exitCode) { Add-Diagnostic 'conformance-exit-missing' }
    if ($diagnostics.Count -eq 0) { $completed = $true }; return
  }
  if (-not $env:NAN_API_KEY) { Fail 'live mode requires explicit provider key' }; Set-Location $workspace; New-Item -ItemType Directory -Path (Join-Path $workspace 'home') | Out-Null; $env:HOME = Join-Path $workspace 'home'; $env:NAN_HARNESS_CONFIG_DIR = Join-Path $workspace 'nan-state'; $usage = Join-Path $workspace 'usage-evidence.json'; $env:NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE = $usage
  $marker = 'NAN_CANARY_READ_' + [guid]::NewGuid().ToString('N'); $readTarget = Join-Path $workspace 'read-target.txt'; Set-Content -LiteralPath $readTarget -Value $marker -NoNewline; $prompt = "Use the available file-reading tool to read '$readTarget'. Include the exact file content, then reply exactly NAN_CANARY_OK. Do not answer before the tool succeeds."
  $stageNow = 'harness-run'
  switch ($Harness) {
    'claude-code' { Run-Native @('claude','--model',$Model,'--','-p',$prompt,'--output-format','stream-json','--verbose','--no-session-persistence','--max-turns','4','--tools','Read','--allowedTools','Read'); if (-not (Has-Text '"name":"Read"')) { Fail 'tool evidence missing' } }
    'codex' { $target=Join-Path $workspace 'codex-tool.txt'; $p="Use exec_command to run printf NAN_CODEX_TOOL_OK > '$target'. After the command succeeds, reply exactly NAN_CANARY_OK."; Run-Native @('codex','--model',$Model,'--','exec','--skip-git-repo-check','--ephemeral','--json','--dangerously-bypass-approvals-and-sandbox',$p); Read-Exact $target 'NAN_CODEX_TOOL_OK' }
    'opencode' { Run-Native @('opencode','--model',$Model,'--','run','--pure','--format','json','--auto',$prompt); if (-not (Has-Regex '"tool"\s*:\s*"read"|"read"')) { Fail 'tool evidence missing' } }
    'hermes' { $target=Join-Path $workspace 'hermes-tool.txt'; $p="You must call write_file exactly once to create '$target' with exactly NAN_HERMES_TOOL_OK. Do not reply before the tool succeeds. Then reply exactly NAN_CANARY_OK."; $env:BFL_API_KEY='';$env:ELEVENLABS_API_KEY='';$env:FAL_KEY='';$env:OPENAI_API_KEY='';$env:XAI_API_KEY=''; Run-Native @('hermes','--model',$Model,'--','chat','--query',$p,'--toolsets','file','--quiet','--yolo','--safe-mode','--source','tool','--max-turns','5'); Read-Exact $target 'NAN_HERMES_TOOL_OK' }
    'pi' { Run-Native @('pi','--model',$Model,'--','--mode','json','--print','--no-session','--no-extensions','--no-skills','--no-prompt-templates','--no-themes','--no-context-files','--tools','read',$prompt); if (-not (Has-Regex '"toolName"\s*:\s*"read"|"read"')) { Fail 'tool evidence missing' } }
    'omp' { Run-Native @('omp','--model',$Model,'--','--mode','json','--print','--no-session','--no-extensions','--no-skills','--no-rules','--no-lsp','--no-title','--tools','read',$prompt); if (-not (Has-Regex '"toolName"\s*:\s*"read"|"read"')) { Fail 'tool evidence missing' } }
    'prime-agent' { $target=Join-Path $workspace 'prime-tool.txt'; $p="Use the ipython tool to write exactly NAN_PRIME_TOOL_OK to '$target'. After it succeeds, reply exactly NAN_CANARY_OK."; Run-Native @('prime','--model',$Model,'--','--mode','json','--print','--no-session','--no-extensions','--no-skills','--no-prompt-templates','--no-themes','--no-context-files','--tools','ipython',$p); Read-Exact $target 'NAN_PRIME_TOOL_OK' }
    'deepseek-harness' { $target=Join-Path $workspace 'deepseek-tool.txt'; $p="Use the write tool to create '$target' with exactly NAN_DEEPSEEK_TOOL_OK. After the tool succeeds, reply exactly NAN_CANARY_OK."; $env:DSH_PERMISSION_MODE='danger-full-access'; Run-Native @('dsh','--model',$Model,'--','--profile','headless',$p); Read-Exact $target 'NAN_DEEPSEEK_TOOL_OK' }
    'openclaw' { Run-Native @('openclaw','--model',$Model,'--','agent','--local','--session-id','nan-harness-canary','--message',$prompt,'--json'); try {$j=Get-Content -Raw $stdout | ConvertFrom-Json} catch { Fail 'openclaw output invalid' }; if ($j.meta.toolSummary.calls -le 0 -or $j.meta.toolSummary.failures -ne 0 -or @($j.meta.toolSummary.tools) -notcontains 'read') { Fail 'tool evidence missing' } }
    'cline' { Run-Native @('cline','--model',$Model,'--','--json','--timeout','120',$prompt); if (-not (Has-Text 'read_files')) { Fail 'tool evidence missing' } }
    'qwen-code' { Run-Native @('qwen','--model',$Model,'--','--safe-mode','--prompt',$prompt,'--output-format','stream-json'); if (-not (Has-Text '"name":"read_file"')) { Fail 'tool evidence missing' } }
    'kimi-code' { Run-Native @('kimi','--model',$Model,'--','--prompt',$prompt,'--output-format','stream-json'); if (-not (Has-Text 'Read')) { Fail 'tool evidence missing' } }
    'aider' { $target=Join-Path $workspace 'edit-target.txt'; Set-Content $target 'AIDER_CANARY_BEFORE'; Run-Native @('aider','--model',$Model,'--','--message','Replace the entire file content with exactly AIDER_CANARY_TOOL_OK. After the edit succeeds, respond with the standalone token NAN_CANARY_OK as the final line of your response.','--yes-always','--no-auto-commits','--no-git','--edit-format','whole','--no-show-model-warnings','--no-check-update','--map-tokens','0','edit-target.txt'); Read-Exact $target 'AIDER_CANARY_TOOL_OK' }
    'goose' { Run-Native @('goose','--model',$Model,'--','run','--no-profile','--no-session','--with-builtin','developer','--output-format','json','--text',$prompt); if (-not (Has-Regex '"name"\s*:\s*"shell"')) { Fail 'tool evidence missing' } }
    'fx' { Run-Native @('fx','--model',$Model,'--','ask','--yolo','--no-save','--no-color',$prompt); if (-not (Has-Text "Reading $readTarget")) { Fail 'tool evidence missing' } }
  }
  $stageNow = 'read-marker'; if ($Harness -notin @('codex','hermes','prime-agent','deepseek-harness','openclaw','aider') -and -not (Has-Text $marker)) { Fail 'read marker missing' }
  $stageNow = 'completion-marker'; if (-not (Has-Text 'NAN_CANARY_OK')) { Fail 'completion marker missing' }; $stageNow = 'bridge-sentinel'; if (Has-Text 'NH-BRIDGE-') { Fail 'bridge sentinel observed' }
  $stageNow = 'usage-evidence'; try {$u=Get-Content -Raw $usage | ConvertFrom-Json} catch { Fail 'usage evidence invalid' }; if ($u.schemaVersion -ne 1 -or $u.status -ne 'observed') { Fail 'usage evidence invalid' }
  $stageNow = 'usage-summary'; if (-not (Has-Regex '^(🔥 Tokens burned — this session|NaN usage \(')) { Fail 'usage summary missing' }; if ($null -eq $exitCode) { Add-Diagnostic 'live-exit-missing'; throw 'live child exit evidence missing' }; $completed = $true
} catch { exit 1 } finally { if ($workspace) { Remove-Item -LiteralPath $workspace -Recurse -Force -ErrorAction SilentlyContinue }; if ($completed) { Write-Result 'complete' 'passed' } else { Write-Result $stageNow 'failed' } }
