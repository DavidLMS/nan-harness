[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][ValidateSet('claude-code','codex','opencode','hermes','pi','omp','prime-agent','deepseek-harness','openclaw','cline','qwen-code','kimi-code','aider','goose','fx')][string]$Harness,
  [Parameter(Mandatory=$true)][ValidateSet('version-doctor','deterministic-contract','live-tool')][string]$Stage,
  [string]$Model = 'qwen3.6', [Parameter(Mandatory=$true)][string]$NanBinary,
  [Parameter(Mandatory=$true)][string]$Canary, [Parameter(Mandatory=$true)][string]$Version
)
$ErrorActionPreference = 'Stop'; $stageNow = $Stage; $workspace = $null; $stdout = $null; $stderr = $null; $completed = $false; $markerPath = $env:NAN_CANARY_PROBE_RESULT; $diagnostics = New-Object System.Collections.Generic.List[string]; $exitCode = $null; $doctorVersion = $null; $doctorExpectedVersion = $null; $doctorReason = $null; $doctorSchemaReason = $null; $discoveryCode = $null; $inventoryFailureReasons = $null; $inventoryProcess = $null; $failedScenarios = $null
$knownDiagnostics = @('doctor-child-launch','doctor-exit-nonzero','doctor-output-invalid','doctor-schema-invalid','doctor-version-missing','doctor-version-invalid','doctor-version-mismatch','doctor-exit-missing','conformance-child-launch','conformance-exit-nonzero','conformance-output-invalid','conformance-schema-invalid','conformance-scenario-missing','conformance-scenario-failed','conformance-inventory-failed','conformance-inventory-operational-failed','conformance-check-invalid','conformance-exit-missing','live-child-launch','live-exit-nonzero','live-exit-missing','live-credential-missing','live-tool-evidence-missing','live-read-marker-missing','live-completion-marker-missing','live-bridge-sentinel','live-usage-invalid','live-usage-summary-missing','probe-unexpected-failure')
function Add-Diagnostic([string]$Code) { if ($knownDiagnostics -contains $Code -and -not $diagnostics.Contains($Code)) { [void]$diagnostics.Add($Code) } }
function Write-Result([string]$resultStage, [string]$status) {
  if (-not $markerPath) { return }; $parent = Split-Path -Parent $markerPath
  $value = [ordered]@{ schemaVersion = 2; stage = $resultStage; status = $status; diagnostics = @($diagnostics.ToArray()) }
  if ($null -ne $exitCode) { $value.exitCode = [int]$exitCode }
  if ($null -ne $doctorVersion) { $value.doctorVersion = $doctorVersion }
  if ($null -ne $doctorExpectedVersion) { $value.doctorExpectedVersion = $doctorExpectedVersion }
  if ($null -ne $doctorReason) { $value.doctorReason = $doctorReason }
  if ($null -ne $doctorSchemaReason) { $value.doctorSchemaReason = $doctorSchemaReason }
  if ($null -ne $discoveryCode) { $value.discoveryCode = $discoveryCode }
  if ($null -ne $inventoryFailureReasons) { $value.inventoryFailureReasons = @($inventoryFailureReasons) }
  if ($null -ne $inventoryProcess) { $value.inventoryProcess = $inventoryProcess }
  if ($null -ne $failedScenarios) { $value.failedScenarios = @($failedScenarios) }
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
  $previousErrorAction = $ErrorActionPreference
  try {
    $command = Get-Command -Name $NanBinary -CommandType Application,ExternalScript -ErrorAction Stop
    # Windows PowerShell 5.1 turns native stderr into error records. A warning
    # must not interrupt the child or replace its real exit status.
    $ErrorActionPreference = 'Continue'
    $global:LASTEXITCODE = $null
    & $command @Arguments 1> $stdout 2> $stderr
    $script:exitCode = $global:LASTEXITCODE
  }
  catch { Add-Diagnostic 'live-child-launch'; $script:exitCode = -1; throw }
  finally { $ErrorActionPreference = $previousErrorAction }
  if ($null -eq $script:exitCode) { Add-Diagnostic 'live-exit-missing'; throw 'harness exit code unavailable' }
  if ($script:exitCode -ne 0) { Add-Diagnostic 'live-exit-nonzero'; throw 'harness command failed' }
}
function Has-Text([string]$Pattern) { return [bool](Select-String -LiteralPath @($stdout, $stderr) -Pattern $Pattern -SimpleMatch -Quiet -ErrorAction SilentlyContinue) }
function Has-Regex([string]$Pattern) { return [bool](Select-String -LiteralPath @($stdout, $stderr) -Pattern $Pattern -Quiet -ErrorAction SilentlyContinue) }
function Read-Exact([string]$Path, [string]$Expected) { if (-not (Test-Path -LiteralPath $Path)) { Fail 'tool result missing' }; if ((Get-Content -Raw -LiteralPath $Path).TrimEnd("`r", "`n") -cne $Expected) { Fail 'tool result mismatch' } }
function Is-Integer($Value) {
  if ($Value -is [bool]) { return $false }
  return $Value -is [byte] -or $Value -is [sbyte] -or $Value -is [int16] -or $Value -is [uint16] -or
    $Value -is [int32] -or $Value -is [uint32] -or $Value -is [int64] -or $Value -is [uint64]
}
function Is-BoundedInteger($Value, [decimal]$Maximum) {
  return (Is-Integer $Value) -and ([decimal]$Value -ge 0) -and ([decimal]$Value -le $Maximum)
}
function Is-SignedInt32($Value) {
  return (Is-Integer $Value) -and ([decimal]$Value -ge -2147483648) -and ([decimal]$Value -le 2147483647)
}
function Is-DoctorOptionalString([string]$Name, $Value) {
  if ($Value -is [string]) { return $true }
  # PowerShell may materialize ISO-8601 JSON timestamps as DateTime. These
  # two fields are Rust String options; accept only that decoder conversion.
  return $Name -in @('compatibleAt','liveVerifiedAt') -and $Value -is [datetime]
}
function Has-OnlyProperties([object]$Value, [string[]]$Allowed) { return -not (@($Value.PSObject.Properties.Name | Where-Object { $Allowed -notcontains $_ }).Count -gt 0) }
function Validate-InventoryProcess([object]$Value) {
  if ($null -eq $Value -or -not (Has-OnlyProperties $Value @('status','exitCode','osErrorCode','timeoutMilliseconds','cleanupStage','cleanupStream'))) { return $false }
  if ($Value.status -isnot [string]) { return $false }
  $status = $Value.status
  if ($status -notin @('completed','nonzero-exit','launch-error','environment-error','timeout','missing-output','capture-error','cleanup-error')) { return $false }
  if ($null -ne $Value.exitCode -and -not (Is-SignedInt32 $Value.exitCode)) { return $false }
  if ($null -ne $Value.osErrorCode -and -not (Is-BoundedInteger $Value.osErrorCode 4294967295)) { return $false }
  if ($null -ne $Value.timeoutMilliseconds -and -not (Is-BoundedInteger $Value.timeoutMilliseconds 86400000)) { return $false }
  if ($null -ne $Value.cleanupStage -and ($Value.cleanupStage -isnot [string] -or $Value.cleanupStage -notin @('terminate','wait','wait-timeout','capture-timeout'))) { return $false }
  if ($null -ne $Value.cleanupStream -and ($Value.cleanupStream -isnot [string] -or $Value.cleanupStream -notin @('stdout','stderr'))) { return $false }
  if ($status -eq 'completed') { return $null -ne $Value.exitCode -and $Value.exitCode -eq 0 -and $null -eq $Value.osErrorCode -and $null -eq $Value.timeoutMilliseconds -and $null -eq $Value.cleanupStage -and $null -eq $Value.cleanupStream }
  if ($status -eq 'nonzero-exit') { return ($null -eq $Value.osErrorCode -and $null -eq $Value.timeoutMilliseconds -and $null -eq $Value.cleanupStage -and $null -eq $Value.cleanupStream -and ($null -eq $Value.exitCode -or $Value.exitCode -ne 0)) }
  if ($status -in @('launch-error','environment-error')) { return $null -eq $Value.exitCode -and $null -eq $Value.timeoutMilliseconds -and $null -eq $Value.cleanupStage -and $null -eq $Value.cleanupStream }
  if ($status -eq 'timeout') { return $null -eq $Value.exitCode -and $null -eq $Value.osErrorCode -and $null -eq $Value.cleanupStage -and $null -eq $Value.cleanupStream -and $null -ne $Value.timeoutMilliseconds -and $Value.timeoutMilliseconds -gt 0 }
  if ($status -in @('missing-output','capture-error')) { return $null -eq $Value.exitCode -and $null -eq $Value.osErrorCode -and $null -eq $Value.timeoutMilliseconds -and $null -eq $Value.cleanupStage -and $null -eq $Value.cleanupStream }
  return $null -eq $Value.exitCode -and $null -eq $Value.timeoutMilliseconds -and $null -ne $Value.cleanupStage -and $null -ne $Value.cleanupStream
}
function Validate-Conformance([object]$Value, [string]$ExpectedHarness) {
  $valid = $true; $names = @('external-prerequisite','inventory','sentinel','tool-round-trip'); $seen = @{}
  if ($null -eq $Value -or -not (Is-BoundedInteger $Value.schemaVersion 2) -or $Value.schemaVersion -notin @(1,2) -or [string]$Value.harness -cne $ExpectedHarness -or [string]$Value.outcome -notin @('passed','failed') -or $null -eq $Value.scenarios -or -not (Has-OnlyProperties $Value @('schemaVersion','harness','scenarios','outcome','durationMilliseconds','observations','inventoryFailureReasons','inventoryProcess')) -or -not (Is-BoundedInteger $Value.durationMilliseconds 86400000)) { Add-Diagnostic 'conformance-schema-invalid'; return $false }
  if ($null -ne $Value.inventoryProcess -and ($Value.schemaVersion -eq 1 -or -not (Validate-InventoryProcess $Value.inventoryProcess))) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  if ($Value.schemaVersion -eq 1 -and $null -ne $Value.observations -and @($Value.observations).Count -gt 0) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  if ($Value.schemaVersion -eq 2 -and $null -ne $Value.observations) {
    $observations = @($Value.observations); if ($observations.Count -gt 1) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
    foreach ($observation in $observations) { if (-not (Has-OnlyProperties $observation @('kind','fingerprint')) -or [string]$observation.kind -cne 'inventory-drift' -or [string]$observation.fingerprint -notmatch '^[0-9a-fA-F]{64}$') { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false } }
  }
  if ($null -ne $Value.inventoryFailureReasons) {
    $reasons = @($Value.inventoryFailureReasons); $allowedReasons = @('process-failed','marker-missing','provider-failed','provider-shutdown-failed','daemon-cleanup-failed')
    $inventoryFailed = @($Value.scenarios | Where-Object { $_.name -eq 'inventory' -and $_.status -eq 'failed' }).Count -gt 0
    if ($Value.schemaVersion -eq 1 -or $reasons.Count -gt 5 -or -not $inventoryFailed -or @($reasons | Where-Object { $allowedReasons -notcontains [string]$_ }).Count -gt 0 -or @($reasons | Select-Object -Unique).Count -ne $reasons.Count) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  }
  $scenarios = @($Value.scenarios); if ($scenarios.Count -ne 4) { Add-Diagnostic 'conformance-scenario-missing'; $valid = $false }
  foreach ($scenario in $scenarios) {
    $name = [string]$scenario.name; if (-not (Has-OnlyProperties $scenario @('name','status','checks','durationMilliseconds')) -or $name.Length -gt 64 -or $names -notcontains $name -or $seen.ContainsKey($name)) { Add-Diagnostic 'conformance-scenario-missing'; $valid = $false } else { $seen[$name] = $true }
    $allowed = if ($name -eq 'external-prerequisite') { @('passed','skipped','failed') } else { @('passed','failed') }
    if ($allowed -notcontains [string]$scenario.status -or $null -eq $scenario.checks -or @($scenario.checks).Count -lt 1 -or @($scenario.checks).Count -gt 8 -or -not (Is-BoundedInteger $scenario.durationMilliseconds 86400000)) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
    if ([string]$scenario.status -eq 'failed') { Add-Diagnostic 'conformance-scenario-failed'; if ($name -eq 'inventory') { Add-Diagnostic 'conformance-inventory-failed' } }
    foreach ($check in @($scenario.checks)) { if (-not (Has-OnlyProperties $check @('name','status','durationMilliseconds')) -or [string]::IsNullOrEmpty([string]$check.name) -or ([string]$check.name).Length -gt 64 -or [string]$check.status -notin @('passed','failed','skipped') -or -not (Is-BoundedInteger $check.durationMilliseconds 86400000)) { Add-Diagnostic 'conformance-check-invalid'; $valid = $false } }
  }
  if ($Value.outcome -eq 'passed' -and $diagnostics.Contains('conformance-scenario-failed')) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  if ($Value.outcome -eq 'failed' -and -not $diagnostics.Contains('conformance-scenario-failed')) { Add-Diagnostic 'conformance-schema-invalid'; $valid = $false }
  return $valid
}
try {
  $workspace = Join-Path ([System.IO.Path]::GetTempPath()) ('nan-canary-' + [guid]::NewGuid().ToString('N')); New-Item -ItemType Directory -Path $workspace -Force | Out-Null
  $stdout = Join-Path $workspace 'harness-output.txt'; $stderr = Join-Path $workspace 'harness-stderr.txt'
  if ($Stage -eq 'version-doctor') {
    if ($Version -match '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$') { $doctorExpectedVersion = $Version }
    try { & $NanBinary 'doctor' $Harness '--allow-unsupported' '--allow-untested' '--json' 1> $stdout 2> $stderr; $exitCode = $LASTEXITCODE } catch { Add-Diagnostic 'doctor-child-launch'; $exitCode = -1 }
    if ($exitCode -ne 0) { Add-Diagnostic 'doctor-exit-nonzero' }
    try { $value = Get-Content -Raw $stdout | ConvertFrom-Json } catch { Add-Diagnostic 'doctor-output-invalid'; $value = $null }
    if ($null -ne $value) {
      $allowed = @('schemaVersion','offline','harness','level','installed','version','minimumSupportedVersion','lastCompatibleVersion','compatibleAt','lastLiveVerifiedVersion','liveVerifiedAt','compatibility','warnings','errorCode','safeToShare')
      $optionalStrings = @('version','minimumSupportedVersion','lastCompatibleVersion','compatibleAt','lastLiveVerifiedVersion','liveVerifiedAt','compatibility','errorCode')
      $badOptional = @($optionalStrings | Where-Object { $null -ne $value.$_ -and -not (Is-DoctorOptionalString $_ $value.$_) }).Count -gt 0
      $unknownFields = @($value.PSObject.Properties.Name | Where-Object { $allowed -notcontains $_ }).Count -gt 0
      $missingFields = @('schemaVersion','offline','harness','level','installed','warnings','safeToShare' | Where-Object { $null -eq $value.$_ }).Count -gt 0
      $badTypes = -not (Is-BoundedInteger $value.schemaVersion 8) -or -not ($value.offline -is [bool]) -or -not ($value.installed -is [bool]) -or -not ($value.safeToShare -is [bool]) -or ($null -ne $value.warnings -and @($value.warnings | Where-Object { $_ -isnot [string] }).Count -gt 0) -or $badOptional
      if ($unknownFields) { $doctorSchemaReason = 'unknown-field' }
      elseif ($missingFields) { $doctorSchemaReason = 'required-field' }
      elseif ($badTypes) { $doctorSchemaReason = 'field-type' }
      elseif ($value.schemaVersion -ne 8 -or [string]$value.harness -cne $Harness -or [string]$value.level -notin @('ok','warning','info','error')) { $doctorSchemaReason = 'field-value' }
      if ($null -ne $doctorSchemaReason) { Add-Diagnostic 'doctor-schema-invalid' }
      if ($null -eq $value.version) { Add-Diagnostic 'doctor-version-missing'; $doctorReason = 'missing' }
      elseif ($value.version -isnot [string] -or [string]$value.version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$') { Add-Diagnostic 'doctor-version-invalid'; $doctorReason = 'invalid' }
      else { $doctorVersion = [string]$value.version; if ([string]$value.version -cne $Version) { Add-Diagnostic 'doctor-version-mismatch'; $doctorReason = 'mismatch' } }
      $discoveryCodes = @('NH-DISCOVERY-001','NH-DISCOVERY-002','NH-DISCOVERY-003','NH-DISCOVERY-004','NH-DISCOVERY-005','NH-DISCOVERY-006','NH-DISCOVERY-007')
      if ($null -ne $value.errorCode -and $discoveryCodes -contains [string]$value.errorCode) { $discoveryCode = [string]$value.errorCode; if ($null -eq $doctorReason) { $doctorReason = 'discovery-error' } }
    }
    if ($diagnostics.Count -eq 0 -and $null -eq $exitCode) { Add-Diagnostic 'doctor-exit-missing' }
    if ($diagnostics.Count -eq 0) { $completed = $true; return }; exit 1
  }
  if ($Stage -eq 'deterministic-contract') {
    try { & $Canary 'conformance' '--nan-harness' $NanBinary '--harness' $Harness '--json' 1> $stdout 2> $stderr; $exitCode = $LASTEXITCODE } catch { Add-Diagnostic 'conformance-child-launch'; $exitCode = -1 }
    if ($exitCode -ne 0) { Add-Diagnostic 'conformance-exit-nonzero' }
    try { $value = Get-Content -Raw $stdout | ConvertFrom-Json } catch { Add-Diagnostic 'conformance-output-invalid'; $value = $null }
    if ($null -eq $value) { Add-Diagnostic 'conformance-schema-invalid' } else {
      $valueValid = Validate-Conformance $value $Harness
      $allowedReasons = @('process-failed','marker-missing','provider-failed','provider-shutdown-failed','daemon-cleanup-failed')
      if ($null -ne $value.inventoryFailureReasons) {
        $inventoryFailureReasons = @($value.inventoryFailureReasons)
        if ($inventoryFailureReasons.Count -gt 5 -or @($inventoryFailureReasons | Where-Object { $allowedReasons -notcontains [string]$_ }).Count -gt 0 -or @($inventoryFailureReasons | Select-Object -Unique).Count -ne $inventoryFailureReasons.Count) { Add-Diagnostic 'conformance-schema-invalid'; $inventoryFailureReasons = $null }
      }
      if ($valueValid -and $null -ne $value.inventoryProcess) { $inventoryProcess = $value.inventoryProcess }
      if ($null -ne $value.scenarios) {
        # Closed scenario names only: the report uses them, and knowing which contract
        # failed is what makes a real conformance failure diagnosable.
        $knownScenarios = @('external-prerequisite','inventory','sentinel','tool-round-trip')
        $failed = @($value.scenarios | Where-Object {
          $null -ne $_ -and [string]$_.status -ceq 'failed' -and $knownScenarios -contains [string]$_.name
        } | ForEach-Object { [string]$_.name } | Select-Object -Unique)
        if ($failed.Count -gt 0) { $failedScenarios = $failed }
      }
      if ($diagnostics.Contains('conformance-inventory-failed')) { Add-Diagnostic 'conformance-inventory-operational-failed' }
    }
    if ($diagnostics.Count -eq 0 -and $null -eq $exitCode) { Add-Diagnostic 'conformance-exit-missing' }
    if ($diagnostics.Count -eq 0) { $completed = $true; return }; exit 1
  }
  if (-not $env:NAN_API_KEY) { Fail 'live mode requires explicit provider key' }; Set-Location $workspace; New-Item -ItemType Directory -Path (Join-Path $workspace 'home') | Out-Null; $env:HOME = Join-Path $workspace 'home'; $env:NAN_HARNESS_CONFIG_DIR = Join-Path $workspace 'nan-state'; $usage = Join-Path $workspace 'usage-evidence.json'; $env:NAN_HARNESS_INTERNAL_CANARY_USAGE_FILE = $usage
  $marker = 'NAN_CANARY_READ_' + [guid]::NewGuid().ToString('N'); $readTarget = Join-Path $workspace 'read-target.txt'; Set-Content -LiteralPath $readTarget -Value $marker -NoNewline; $prompt = "Use the available file-reading tool to read '$readTarget'. Include the exact file content, then reply exactly NAN_CANARY_OK. Do not answer before the tool succeeds."
  $stageNow = 'harness-run'
  switch ($Harness) {
    'claude-code' { Run-Native @('claude','--model',$Model,'--','-p',$prompt,'--output-format','stream-json','--verbose','--no-session-persistence','--max-turns','4','--tools','Read','--allowedTools','Read'); if (-not (Has-Text '"name":"Read"')) { Fail 'tool evidence missing' } }
    'codex' { $target=Join-Path $workspace 'codex-tool.txt'; $quotedTarget = $target.Replace("'", "''"); $p="Use exec_command to run powershell -NoProfile -Command `"Set-Content -NoNewline -LiteralPath '$quotedTarget' -Value 'NAN_CODEX_TOOL_OK'`". After the command succeeds, reply exactly NAN_CANARY_OK."; Run-Native @('codex','--model',$Model,'--','exec','--skip-git-repo-check','--ephemeral','--json','--dangerously-bypass-approvals-and-sandbox',$p); Read-Exact $target 'NAN_CODEX_TOOL_OK' }
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
  $stageNow = 'usage-evidence'; try {$u=Get-Content -Raw $usage | ConvertFrom-Json} catch { Fail 'usage evidence invalid' }; if (-not (Is-BoundedInteger $u.schemaVersion 1) -or $u.schemaVersion -ne 1 -or $u.status -ne 'observed') { Fail 'usage evidence invalid' }
  $stageNow = 'usage-summary'; if (-not (Has-Regex '^(🔥 Tokens burned — this session|NaN usage \()')) { Fail 'usage summary missing' }; if ($null -eq $exitCode) { Add-Diagnostic 'live-exit-missing'; throw 'live child exit evidence missing' }; $completed = $true
} catch { exit 1 } finally {
  if ($workspace) { Remove-Item -LiteralPath $workspace -Recurse -Force -ErrorAction SilentlyContinue }
  # A failure always carries one closed code, so an unexpected stage failure is never
  # published as an unattributable marker.
  if (-not $completed -and $diagnostics.Count -eq 0) { Add-Diagnostic 'probe-unexpected-failure' }
  if ($completed) { Write-Result 'complete' 'passed' } else { Write-Result $stageNow 'failed' }
}
