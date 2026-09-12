param([Parameter(Mandatory = $true)][string]$Harness)
$ErrorActionPreference = 'Stop'

# Native Windows installers. Keep this list in lockstep with the CLI catalog;
# Git Bash/WSL is not used for installation on the hosted Windows runner.
switch ($Harness) {
  'claude-code' { npm install --global '@anthropic-ai/claude-code@latest' }
  'codex' { npm install --global '@openai/codex@latest' }
  'opencode' { npm install --global 'opencode-ai@latest' }
  'pi' { npm install --global --ignore-scripts '@earendil-works/pi-coding-agent@latest' }
  'deepseek-harness' { npm install --global '@deepseek-ai/dsh@latest' }
  'openclaw' { npm install --global 'openclaw@latest' }
  'cline' { npm install --global 'cline@latest' }
  'qwen-code' { npm install --global '@qwen-code/qwen-code@latest' }
  'aider' { uv tool install --python 3.12 aider-chat }
  'omp' {
    $installer = Join-Path $env:TEMP 'omp-install.ps1'
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/can1357/oh-my-pi/main/scripts/install.ps1' -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer -Binary
  }
  'goose' {
    $installer = Join-Path $env:TEMP 'goose-install.ps1'
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/block/goose/main/download_cli.ps1' -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer
  }
  'hermes' {
    $installer = Join-Path $env:TEMP 'hermes-install.ps1'
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1' -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer -NoVenv -SkipSetup -Branch main
  }
  'prime-agent' {
    $installer = Join-Path $env:TEMP 'prime-agent-install.sh'
    Invoke-WebRequest -Uri 'https://app.primeintellect.ai/prime-agent/install.sh' -OutFile $installer
    & bash $installer
  }
  'kimi-code' {
    $installer = Join-Path $env:TEMP 'kimi-install.ps1'
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/scripts/install.ps1' -OutFile $installer
    & powershell -NoProfile -ExecutionPolicy Bypass -File $installer
  }
  'fx' {
    throw "upstream blocker: official fx support is macOS/Linux only (https://fx.sh/docs/getting-started/installation)"
  }
  default {
    throw "unknown CLI harness '$Harness'"
  }
}
