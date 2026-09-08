# Official system installers are allowed only in disposable hosted Windows jobs.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('chatgpt-desktop', 'claude-desktop', 'hermes-desktop', 'pen-desktop', 'zed-desktop')]
    [string] $App
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'System installation requires a disposable GitHub-hosted Windows runner.'
}
if (Test-Path Env:NAN_API_KEY) { throw 'Remove NAN_API_KEY before preparing Desktop installations.' }
$directory = Join-Path $env:RUNNER_TEMP ('desktop-install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory | Out-Null
$sources = @{
    'chatgpt-desktop' = @('https://persistent.oaistatic.com/codex-app-prod/ChatGPT-x64.msix', 'msix', 'OpenAI.ChatGPT')
    'claude-desktop' = @('https://claude.ai/api/desktop/win32/x64/msix', 'msix', 'Claude')
    'hermes-desktop' = @('https://hermes-assets.nousresearch.com/Hermes-Setup.exe', 'nsis', 'Hermes')
    'pen-desktop' = @('https://www.pen.dev/download/Pen-win-x64.exe', 'nsis', 'Pen')
    'zed-desktop' = @('https://github.com/zed-industries/zed/releases/latest/download/Zed-x86_64.exe', 'inno', 'Zed')
}
$source = $sources[$App]
$extension = if ($source[1] -eq 'msix') { '.msix' } else { '.exe' }
$package = Join-Path $directory ('installer' + $extension)
# curl enforces HTTPS across redirects and bounds both size and elapsed time.
& curl.exe --proto '=https' --proto-redir '=https' --fail --location --silent --show-error --max-time 300 --max-filesize 2147483648 $source[0] --output $package
if ($LASTEXITCODE -ne 0) { throw 'The official Desktop package download failed.' }
$receipt = @{ app = $App; source = $source[0]; sha256 = (Get-FileHash $package -Algorithm SHA256).Hash.ToLowerInvariant(); retention = 'disposable-runner'; status = 'prepared' }
$receiptPath = Join-Path $directory 'installation.json'
$receipt | ConvertTo-Json -Compress | Set-Content $receiptPath
if ($source[1] -eq 'msix') {
    if (@(Get-AppxPackage -Name $source[2]).Count -ne 0) { throw 'An existing package was left unchanged.' }
    # Registration verifies the publisher's package signature. No all-user provisioning,
    # developer-mode exception, virtualization feature or forced app shutdown is enabled.
    Add-AppxPackage -Path $package -ErrorAction Stop
    $installed = @(Get-AppxPackage -Name $source[2] -ErrorAction Stop)
    if ($installed.Count -ne 1) { throw 'The installed Desktop package is ambiguous.' }
    $receipt.packageFullName = $installed[0].PackageFullName
} else {
    $target = Join-Path (Join-Path $env:LOCALAPPDATA 'Programs') $source[2]
    if (Test-Path $target) { throw 'An existing installation directory was left unchanged.' }
    $start = [Diagnostics.ProcessStartInfo]::new($package)
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($name in @('NAN_API_KEY', 'GH_TOKEN', 'GITHUB_TOKEN', 'GITHUB_ENV', 'GITHUB_OUTPUT', 'GITHUB_PATH', 'GITHUB_STEP_SUMMARY', 'ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN')) {
        $start.Environment.Remove($name) | Out-Null
    }
    if ($source[1] -eq 'inno') {
        foreach ($argument in @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOCLOSEAPPLICATIONS', '/NORESTARTAPPLICATIONS', '/TASKS=', "/DIR=$target")) { $start.ArgumentList.Add($argument) }
    } else {
        # NSIS requires an unquoted /D value at the end, including paths with spaces.
        # Arguments is passed directly to the executable, never through a shell.
        $start.Arguments = '/S /D=' + $target
    }
    $process = [Diagnostics.Process]::Start($start)
    $stdout = $process.StandardOutput.BaseStream.CopyToAsync([IO.Stream]::Null)
    $stderr = $process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
    if (-not $process.WaitForExit(600000)) { $process.Kill($true); throw 'Desktop installation timed out.' }
    $null = $stdout.GetAwaiter().GetResult()
    $null = $stderr.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) { throw 'The official Desktop installer failed.' }
    $receipt.installationDirectory = $target
}
$receipt.status = 'installed'
$receipt | ConvertTo-Json -Compress | Set-Content $receiptPath
Write-Host 'Official Desktop installation prepared; it will be retained until the disposable runner is destroyed.'
