param([Parameter(Mandatory = $true)][string]$Harness)
$ErrorActionPreference = 'Stop'

# GitHub's native Windows image ships Git Bash. The probe itself remains the
# shared, audited contract; PowerShell owns process invocation and credentials
# on Windows, without WSL or a Linux runner.
$script = Join-Path $PSScriptRoot 'probe-harness.sh'
& bash $script $Harness
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
