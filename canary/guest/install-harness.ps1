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
  'aider' { uv tool install --python 3.12 "aider-chat==$Version" }
  'omp' {
    $installer = Join-Path $env:TEMP 'omp-install.ps1'
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/can1357/oh-my-pi/main/scripts/install.ps1' -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer -Binary -Ref "v$Version"
  }
  'goose' {
    $installer = Join-Path $env:TEMP 'goose-install.ps1'
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/block/goose/v$Version/download_cli.ps1" -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer
  }
  'hermes' {
    $installer = Join-Path $env:TEMP 'hermes-install.ps1'
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/NousResearch/hermes-agent/v$Version/scripts/install.ps1" -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer -NoVenv -SkipSetup -Branch "v$Version"
  }
  'prime-agent' {
    $installer = Join-Path $env:TEMP 'prime-agent-install.sh'
    Invoke-WebRequest -Uri 'https://app.primeintellect.ai/prime-agent/install.sh' -OutFile $installer
    & bash $installer --version $Version
  }
  'kimi-code' {
    $installer = Join-Path $env:TEMP 'kimi-install.ps1'
    Invoke-WebRequest -Uri "https://raw.githubusercontent.com/MoonshotAI/kimi-cli/v$Version/scripts/install.ps1" -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer -Version $Version
  }
  'fx' {
    throw "upstream blocker: official fx support is macOS/Linux only (https://fx.sh/docs/getting-started/installation)"
  }
  default {
    throw "unknown CLI harness '$Harness'"
  }
}
