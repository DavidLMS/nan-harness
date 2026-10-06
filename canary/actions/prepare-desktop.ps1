# Install exact frozen Desktop entries on a disposable hosted Windows runner.
param(
    [Parameter(Mandatory = $true)] [string] $Manifest,
    [Parameter(Mandatory = $true)] [string] $Artifacts,
    [Parameter(Mandatory = $true)] [string] $Platform,
    [Parameter(Mandatory = $true)] [string] $Architecture,
    [Parameter(Mandatory = $true)] [string] $Model,
    [Parameter(Mandatory = $true)] [string] $Harnesses
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'System installation requires a disposable GitHub-hosted Windows runner.'
}
if (Test-Path Env:NAN_API_KEY) { throw 'Remove NAN_API_KEY before preparing Desktop installations.' }
$environment = @{}
foreach ($item in [Environment]::GetEnvironmentVariables().GetEnumerator()) { $environment[$item.Key] = $item.Value }
foreach ($name in @('NAN_API_KEY', 'GH_TOKEN', 'GITHUB_TOKEN', 'GITHUB_ENV', 'GITHUB_OUTPUT', 'GITHUB_PATH', 'GITHUB_STEP_SUMMARY', 'ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN')) { $environment.Remove($name) }
$python = Join-Path $PSScriptRoot 'desktop_install.py'
$arguments = @($python, '--manifest', $Manifest, '--artifacts', $Artifacts, '--platform', $Platform, '--architecture', $Architecture, '--model', $Model, '--harnesses', $Harnesses)
$process = Start-Process -FilePath 'python3' -ArgumentList $arguments -Environment $environment -Wait -PassThru -NoNewWindow
if ($process.ExitCode -ne 0) { throw 'Frozen Desktop installation failed.' }
