param([Parameter(Mandatory = $true)][string]$Harness,
      [Parameter(Mandatory = $false)][string]$Version = 'latest')
$ErrorActionPreference = 'Stop'

# Native Windows installers. Keep this list in lockstep with the CLI catalog;
# Git Bash/WSL is not used for installation on the hosted Windows runner.
switch ($Harness) {
  'claude-code' { npm install --global "@anthropic-ai/claude-code@$Version" }
  'codex' { npm install --global "@openai/codex@$Version" }
  'opencode' { npm install --global "opencode-ai@$Version" }
  'pi' { npm install --global --ignore-scripts "@earendil-works/pi-coding-agent@$Version" }
  'deepseek-harness' { npm install --global "@deepseek-ai/dsh@$Version" }
  'openclaw' { npm install --global "openclaw@$Version" }
  'cline' { npm install --global "cline@$Version" }
  'qwen-code' { npm install --global "@qwen-code/qwen-code@$Version" }
  'aider' {
    if (-not (Get-Command uv -ErrorAction SilentlyContinue)) {
      $bootstrap = Join-Path $env:LOCALAPPDATA 'nan-harness-canary-uv'
      python -m venv $bootstrap
      if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
      & "$bootstrap\Scripts\python.exe" -m pip install 'uv==0.11.31'
      if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
      $env:PATH = "$bootstrap\Scripts;$env:PATH"
    }
    $package = if ($Version -eq 'latest') { 'aider-chat' } else { "aider-chat==$Version" }
    uv tool install --python 3.12 $package
  }
  'omp' {
    $installer = Join-Path $env:TEMP 'omp-install.ps1'
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/can1357/oh-my-pi/main/scripts/install.ps1' -OutFile $installer
    $arguments = @('-Binary')
    if ($Version -ne 'latest') { $arguments += @('-Ref', "v$Version") }
    $env:PI_INSTALL_DIR = Join-Path $env:USERPROFILE '.local\bin'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer @arguments
  }
  'goose' {
    $installer = Join-Path $env:TEMP 'goose-install.ps1'
    $ref = if ($Version -eq 'latest') { 'stable' } else { "v$Version" }
    Invoke-WebRequest -Uri "https://github.com/aaif-goose/goose/releases/download/$ref/download_cli.ps1" -OutFile $installer
    $env:GOOSE_VERSION = if ($Version -eq 'latest') { '' } else { $Version }
    $env:CONFIGURE = 'false'
    $env:GOOSE_BIN_DIR = Join-Path $env:USERPROFILE '.local\bin'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer
  }
  'hermes' {
    $installer = Join-Path $env:TEMP 'hermes-install.ps1'
    $ref = if ($Version -eq 'latest') { 'main' } else { "v$Version" }
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/NousResearch/hermes-agent/$ref/scripts/install.ps1" -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer -SkipSetup -Branch $ref
  }
  'prime-agent' {
    # The official installer distributes a checksummed npm tarball on Windows.
    # Install that exact package with native npm, without a Bash/WSL dependency.
    $base = 'https://pub-728493de92a943e2a9b2d17b4719f318.r2.dev'
    if ($Version -eq 'latest') { $Version = ([string](Invoke-RestMethod "$base/stable")).Trim().TrimStart('v') }
    if ($Version -notmatch '^\d+\.\d+\.\d+(-[A-Za-z0-9.-]+)?$') { throw 'Invalid Prime release version' }
    $name = "prime-agent-$Version.tgz"
    $package = Join-Path $env:TEMP $name
    $checksums = [string](Invoke-RestMethod "$base/releases/v$Version/SHA256SUMS")
    $checksumLines = @($checksums -split "`n" | Where-Object { $_ -match "^[a-fA-F0-9]{64}  $([regex]::Escape($name))\s*$" })
    if ($checksumLines.Count -ne 1) { throw 'Prime release checksum is missing or ambiguous' }
    Invoke-WebRequest -Uri "$base/releases/v$Version/$name" -OutFile $package
    if ((Get-FileHash $package -Algorithm SHA256).Hash -ne $checksumLines[0].Substring(0, 64)) { throw 'Prime checksum mismatch' }
    npm install --global $package
  }
  'kimi-code' {
    $installer = Join-Path $env:TEMP 'kimi-install.ps1'
    Invoke-WebRequest -Uri 'https://code.kimi.com/kimi-code/install.ps1' -OutFile $installer
    $env:KIMI_VERSION = if ($Version -eq 'latest') { '' } else { $Version }
    $env:KIMI_NO_MODIFY_PATH = '1'
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer
  }
  'fx' {
    throw "upstream blocker: official fx support is macOS/Linux only (https://fx.sh/docs/getting-started/installation)"
  }
  default {
    throw "unknown CLI harness '$Harness'"
  }
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
