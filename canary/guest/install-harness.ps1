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
    $asset = Join-Path $env:TEMP 'omp-windows-x64.exe'
    Invoke-WebRequest -Uri 'https://github.com/can1357/oh-my-pi/releases/latest/download/omp-windows-x64.exe' -OutFile $asset
    New-Item -ItemType Directory -Force -Path "$HOME\.local\bin" | Out-Null
    Copy-Item $asset "$HOME\.local\bin\omp.exe" -Force
  }
  'goose' {
    $asset = Join-Path $env:TEMP 'goose.exe'
    Invoke-WebRequest -Uri 'https://github.com/block/goose/releases/latest/download/goose_windows_x86_64.exe' -OutFile $asset
    New-Item -ItemType Directory -Force -Path "$HOME\.local\bin" | Out-Null
    Copy-Item $asset "$HOME\.local\bin\goose.exe" -Force
  }
  'hermes' {
    throw "upstream blocker: the official Hermes CLI installer is POSIX-only (https://hermes-agent.nousresearch.com/)"
  }
  'prime-agent' {
    throw "upstream blocker: the official Prime Agent installer documents Linux/macOS only (https://app.primeintellect.ai/prime-agent/install.sh)"
  }
  'kimi-code' {
    throw "upstream blocker: the official Kimi Code installer is currently POSIX-only (https://code.kimi.com/kimi-code/install.sh)"
  }
  'fx' {
    throw "upstream blocker: the official fx installer is currently POSIX-only (https://fx.sh/setup.sh)"
  }
  default {
    throw "unknown CLI harness '$Harness'"
  }
}
