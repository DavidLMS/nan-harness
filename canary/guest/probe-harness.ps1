[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][ValidateSet('claude-code','codex','opencode','hermes','pi','omp','prime-agent','deepseek-harness','openclaw','cline','qwen-code','kimi-code','aider','goose','fx')][string]$Harness,
  [Parameter(Mandatory=$true)][ValidateSet('version-doctor','deterministic-contract','live-tool')][string]$Stage,
  [string]$Model = 'qwen3.6', [Parameter(Mandatory=$true)][string]$NanBinary,
  [Parameter(Mandatory=$true)][string]$Canary, [Parameter(Mandatory=$true)][string]$Version
)
$ErrorActionPreference = 'Stop'; $stageNow = $Stage; $workspace = $null; $stdout = $null; $stderr = $null; $completed = $false; $markerPath = $env:NAN_CANARY_PROBE_RESULT
function Write-Result([string]$resultStage, [string]$status, [string]$detail = $null) {
  if (-not $markerPath) { return }; $parent = Split-Path -Parent $markerPath
  $value = [ordered]@{ schemaVersion = 1; stage = $resultStage; status = $status }; if ($detail) { $value.diagnostic = $detail }
  $tmp = Join-Path $parent ('.probe-result.' + [guid]::NewGuid().ToString('N'))
  # Windows PowerShell 5.1 supports UTF-8 (with BOM); JSON parsing is encoding-aware.
  try { $value | ConvertTo-Json -Compress | Set-Content -LiteralPath $tmp -Encoding UTF8; Move-Item -LiteralPath $tmp -Destination $markerPath -Force }
  finally { if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue } }
}
function Fail([string]$message) { throw $message }
function Run-Native([string[]]$Arguments) { & $NanBinary @Arguments 1> $stdout 2> $stderr; if ($LASTEXITCODE -ne 0) { Fail 'harness command failed' } }
function Has-Text([string]$Pattern) { return [bool](Select-String -LiteralPath @($stdout, $stderr) -Pattern $Pattern -SimpleMatch -Quiet -ErrorAction SilentlyContinue) }
function Has-Regex([string]$Pattern) { return [bool](Select-String -LiteralPath @($stdout, $stderr) -Pattern $Pattern -Quiet -ErrorAction SilentlyContinue) }
function Read-Exact([string]$Path, [string]$Expected) { if (-not (Test-Path -LiteralPath $Path)) { Fail 'tool result missing' }; if ((Get-Content -Raw -LiteralPath $Path).TrimEnd("`r", "`n") -cne $Expected) { Fail 'tool result mismatch' } }
try {
  $workspace = Join-Path ([System.IO.Path]::GetTempPath()) ('nan-canary-' + [guid]::NewGuid().ToString('N')); New-Item -ItemType Directory -Path $workspace -Force | Out-Null
  $stdout = Join-Path $workspace 'harness-output.txt'; $stderr = Join-Path $workspace 'harness-stderr.txt'
  if ($Stage -eq 'version-doctor') {
    & $NanBinary 'doctor' $Harness '--allow-unsupported' '--allow-untested' '--json' 1> $stdout 2> $stderr; if ($LASTEXITCODE -ne 0) { Fail 'nan doctor failed' }
    try { $value = Get-Content -Raw $stdout | ConvertFrom-Json } catch { Fail 'nan doctor output invalid' }; if (-not $value.version -or [string]$value.version -cne $Version) { Fail 'nan doctor version mismatch' }; $completed = $true; return
  }
  if ($Stage -eq 'deterministic-contract') {
    & $Canary 'conformance' '--nan-harness' $NanBinary '--harness' $Harness '--json' 1> $stdout 2> $stderr; if ($LASTEXITCODE -ne 0) { Fail 'conformance command failed' }
    try { $value = Get-Content -Raw $stdout | ConvertFrom-Json } catch { Fail 'conformance output invalid' }
    if ([string]$value.harness -ne $Harness -or [string]$value.outcome -ne 'passed' -or [int]$value.schemaVersion -notin @(1,2) -or @($value.scenarios).Count -ne 4) { Fail 'conformance contract failed' }
    $expected = @('external-prerequisite','inventory','sentinel','tool-round-trip'); $actual = @($value.scenarios | ForEach-Object { [string]$_.name } | Sort-Object)
    if (Compare-Object -ReferenceObject $expected -DifferenceObject $actual) { Fail 'conformance scenario set incomplete' }
    foreach ($scenario in $value.scenarios) { if (@($scenario.checks).Count -lt 1) { Fail 'conformance checks missing' }; if ([string]$scenario.name -eq 'external-prerequisite') { if ([string]$scenario.status -notin @('passed','skipped')) { Fail 'conformance prerequisite failed' } } elseif ([string]$scenario.name -eq 'inventory') { if ([string]$scenario.status -notin @('passed','failed')) { Fail 'conformance inventory invalid' } } elseif ([string]$scenario.status -ne 'passed') { Fail 'conformance scenario failed' } }
    $completed = $true; return
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
  $stageNow = 'usage-summary'; if (-not (Has-Regex '^(🔥 Tokens burned — this session|NaN usage \(')) { Fail 'usage summary missing' }; $completed = $true
} catch { exit 1 } finally { if ($workspace) { Remove-Item -LiteralPath $workspace -Recurse -Force -ErrorAction SilentlyContinue }; if ($completed) { Write-Result 'complete' 'passed' } else { Write-Result $stageNow 'failed' } }
