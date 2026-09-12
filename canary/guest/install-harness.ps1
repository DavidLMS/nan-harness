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
  default {
    throw "The official native Windows installer for '$Harness' is not configured"
  }
}
