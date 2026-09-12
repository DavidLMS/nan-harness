param([Parameter(Mandatory = $true)][string]$Harness,
      [Parameter(Mandatory = $false)][string]$Version = 'latest',
      [Parameter(Mandatory = $false)][string]$Ref = '')
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 renders download progress synchronously and very slowly.
$ProgressPreference = 'SilentlyContinue'

# Only commit-pinned harnesses accept a frozen 40-hex source commit.
if ($Ref) {
  if ($Harness -ne 'hermes' -or $Version -eq 'latest' -or $Ref -cnotmatch '^[0-9a-f]{40}$') {
    throw 'an installer ref must be a frozen Hermes source commit'
  }
} elseif ($Harness -eq 'hermes' -and $Version -ne 'latest') {
  throw 'an exact Hermes version requires its frozen source commit'
}

function Get-OfficialText([string]$Uri) {
  # Invoke-RestMethod can return bytes for untyped CDN objects; read a file instead.
  $path = Join-Path $env:TEMP ([guid]::NewGuid().ToString('N'))
  try {
    Invoke-WebRequest -Uri $Uri -OutFile $path -UseBasicParsing -TimeoutSec 120
    return [System.IO.File]::ReadAllText($path)
  } finally {
    Remove-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
  }
}

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
    # The installer at the release tag resolves that tag's omp-windows-<arch>.exe.
    $scriptRef = if ($Version -eq 'latest') { 'main' } else { "v$Version" }
    $installer = Join-Path $env:TEMP 'omp-install.ps1'
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/can1357/oh-my-pi/$scriptRef/scripts/install.ps1" -OutFile $installer -UseBasicParsing
    $arguments = @('-Binary')
    if ($Version -ne 'latest') { $arguments += @('-Ref', "v$Version") }
    $env:PI_INSTALL_DIR = Join-Path $env:USERPROFILE '.local\bin'
    & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $installer @arguments
  }
  'goose' {
    # Releases publish download_cli.sh only; the official PowerShell installer
    # lives in the repository, so read it from the exact release tag.
    $scriptRef = if ($Version -eq 'latest') { 'main' } else { "v$Version" }
    $installer = Join-Path $env:TEMP 'goose-install.ps1'
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/aaif-goose/goose/$scriptRef/download_cli.ps1" -OutFile $installer -UseBasicParsing
    $env:GOOSE_VERSION = if ($Version -eq 'latest') { '' } else { $Version }
    $env:CONFIGURE = 'false'
    $env:GOOSE_BIN_DIR = Join-Path $env:USERPROFILE '.local\bin'
    & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $installer
  }
  'hermes' {
    $installer = Join-Path $env:TEMP 'hermes-install.ps1'
    $arguments = @('-SkipSetup', '-NonInteractive')
    if ($Version -eq 'latest') {
      $scriptUri = 'https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1'
    } else {
      # Release tags are dates, not the product version. A fresh main clone
      # already contains the frozen commit, so the pin must be forced.
      $scriptUri = "https://raw.githubusercontent.com/NousResearch/hermes-agent/$Ref/scripts/install.ps1"
      $arguments += @('-Commit', $Ref, '-ForceCommit')
    }
    Invoke-WebRequest -Uri $scriptUri -OutFile $installer -UseBasicParsing
    & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $installer @arguments
  }
  'prime-agent' {
    # install.sh uses its checksummed npm package whenever no compiled archive
    # exists for the platform, which is the case for Windows. Mirror that path
    # with native npm, without a Bash/WSL dependency.
    $base = 'https://pub-728493de92a943e2a9b2d17b4719f318.r2.dev'
    if ($Version -eq 'latest') { $Version = (Get-OfficialText "$base/stable").Trim().TrimStart('v') }
    if ($Version -notmatch '^\d+\.\d+\.\d+(-[A-Za-z0-9.-]+)?$') { throw 'Invalid Prime release version' }
    $name = "prime-agent-$Version.tgz"
    $package = Join-Path $env:TEMP $name
    $checksums = Get-OfficialText "$base/releases/v$Version/SHA256SUMS"
    $checksumLines = @($checksums -split "`n" | Where-Object { $_ -match "^[a-fA-F0-9]{64}  $([regex]::Escape($name))\s*$" })
    if ($checksumLines.Count -ne 1) { throw 'Prime release checksum is missing or ambiguous' }
    Invoke-WebRequest -Uri "$base/releases/v$Version/$name" -OutFile $package -UseBasicParsing
    if ((Get-FileHash $package -Algorithm SHA256).Hash -ne $checksumLines[0].Substring(0, 64)) { throw 'Prime checksum mismatch' }
    # A noninteractive install.sh prepares search tools and the Python kernel
    # used by the live ipython probe.
    $env:PRIME_AGENT_BOOTSTRAP_TOOLS_ON_INSTALL = '1'
    $env:PRIME_AGENT_BOOTSTRAP_KERNEL_ON_INSTALL = '1'
    $env:PRIME_AGENT_INSTALL_UV = '1'
    $npmMajor = [int](([string](npm --version)).Trim().Split('.')[0])
    if ($npmMajor -ge 12) {
      npm install --global --no-fund --no-audit "--allow-remote=all" "--allow-scripts=$package" $package
    } else {
      npm install --global --no-fund --no-audit $package
    }
  }
  'kimi-code' {
    $installer = Join-Path $env:TEMP 'kimi-install.ps1'
    Invoke-WebRequest -Uri 'https://code.kimi.com/kimi-code/install.ps1' -OutFile $installer -UseBasicParsing
    $env:KIMI_VERSION = if ($Version -eq 'latest') { '' } else { $Version }
    $env:KIMI_NO_MODIFY_PATH = '1'
    & powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $installer
  }
  'fx' {
    throw "upstream blocker: official fx support is macOS/Linux only (https://fx.sh/docs/getting-started/installation)"
  }
  default {
    throw "unknown CLI harness '$Harness'"
  }
}
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
