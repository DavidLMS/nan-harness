[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][ValidateSet('claude-code','codex','opencode','hermes','pi','omp','prime-agent','deepseek-harness','openclaw','cline','qwen-code','kimi-code','aider','goose','fx')][string]$Harness,
  [Parameter(Mandatory=$true)][string]$Version,
  [ValidatePattern('^[0-9]+\.[0-9]+$')][string]$PythonVersion = '3.12',
  [string]$Ref = ''
)
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion.Major -lt 7) {
  $pwsh = Get-Command pwsh.exe -ErrorAction Stop
  $forward = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$PSCommandPath,
               '-Harness',$Harness,'-Version',$Version,'-PythonVersion',$PythonVersion)
  if ($Ref) { $forward += @('-Ref',$Ref) }
  & $pwsh.Source @forward
  exit $LASTEXITCODE
}
$cell = (Get-Location).Path
$bin = Join-Path $cell 'bin'
$tmp = Join-Path $cell 'installer-tmp'
New-Item -ItemType Directory -Force -Path $bin,$tmp | Out-Null
$stdoutLog = Join-Path $tmp 'stdout.log'
$stderrLog = Join-Path $tmp 'stderr.log'
$resultPath = Join-Path $cell 'installer-result.json'
$script:InstallDiagnostic = @{}
$script:NpmCode = $null
$script:PipCategory = $null
function Set-InstallDiagnostic {
  param(
    [string]$Subphase,
    [string]$Executable,
    [Nullable[int]]$ExitCode,
    [Nullable[int]]$Win32Error,
    [Nullable[int]]$HttpStatus,
    [string]$AssetReason,
    [string]$NpmCode,
    [string]$PipCategory,
    [string]$ProcessReason
  )
  $script:InstallDiagnostic = @{}
  foreach ($entry in @{
    subphase = $Subphase; executable = $Executable; exitCode = $ExitCode
    win32Error = $Win32Error; httpStatus = $HttpStatus; assetReason = $AssetReason
    npmCode = $NpmCode; pipCategory = $PipCategory; processReason = $ProcessReason
  }.GetEnumerator()) {
    if ($null -ne $entry.Value -and [string]$entry.Value -ne '') { $script:InstallDiagnostic[$entry.Key] = $entry.Value }
  }
}
function Write-InstallerResult([string]$Status, [string]$Reason) {
  $allowed = @('passed','installer-failed','official-asset-missing','official-metadata-probe-failed',
               'official-metadata-no-windows-asset','capability-not-implemented','invalid-frozen-ref','invalid-version')
  if ($allowed -notcontains $Reason) { $Reason = 'installer-failed' }
  $value = @{ schemaVersion = 2; status = $Status; reason = $Reason; diagnostic = $script:InstallDiagnostic } | ConvertTo-Json -Compress
  [IO.File]::WriteAllText($resultPath, $value, [Text.UTF8Encoding]::new($false))
}

function Invoke-Native([string]$File, [string[]]$Arguments, [string]$Executable = 'unknown', [string]$Subphase = 'execute') {
  # ProcessStartInfo.ArgumentList preserves spaces and quotes without cmd.exe.
    Set-InstallDiagnostic $Subphase $Executable $null $null $null $null $null $null $null
  $info = [System.Diagnostics.ProcessStartInfo]::new()
  $info.FileName = $File; $info.WorkingDirectory = $cell; $info.UseShellExecute = $false
  $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
  foreach ($argument in $Arguments) { [void]$info.ArgumentList.Add($argument) }
  $process = [System.Diagnostics.Process]::new(); $process.StartInfo = $info
  try { if (-not $process.Start()) { throw 'native installer process could not start' } }
  catch [System.ComponentModel.Win32Exception] {
    Set-InstallDiagnostic $Subphase $Executable $null $_.Exception.NativeErrorCode $null $null $null $null 'win32-launch-failed'
    throw
  }
  $outTask = $process.StandardOutput.ReadToEndAsync(); $errTask = $process.StandardError.ReadToEndAsync()
  $process.WaitForExit()
  [IO.File]::WriteAllText($stdoutLog, $outTask.Result, [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText($stderrLog, $errTask.Result, [Text.UTF8Encoding]::new($false))
  if ($process.ExitCode -ne 0) {
    $npmCode = $null; $pipCategory = $null
    if ($Executable -in @('npm-node','npm-cmd')) {
      # npm 10+ emits `npm error code`; older npm emits `npm ERR! code`.
      # Match only stable machine codes and command-resolution text; never
      # place provider output, URLs, paths, or credentials in the marker.
      $npmCode = if ($errTask.Result -match '(?im)npm(?: ERR!| error) code (EAI_AGAIN|ECONNRESET|ETIMEDOUT|ENETUNREACH|ENOTFOUND|ECONNREFUSED|E404|EACCES|EPERM|CERT_HAS_EXPIRED|SELF_SIGNED_CERT_IN_CHAIN)') {
        switch ($Matches[1]) {
          'EAI_AGAIN' { 'registry-dns' }; 'ECONNRESET' { 'registry-connection' }; 'ETIMEDOUT' { 'registry-timeout' }
          'ENETUNREACH' { 'registry-unreachable' }; 'ENOTFOUND' { 'registry-dns' }; 'ECONNREFUSED' { 'registry-connection' }
          'E404' { 'package-not-found' }; 'EACCES' { 'permission' }; 'EPERM' { 'permission' }
          default { 'tls-certificate' }
        }
      } elseif ($errTask.Result -match "(?im)'?npm(?:\.cmd)?'? is not recognized|cannot find the path.*npm(?:\.cmd)?") {
        'npm-command-missing'
      } else { 'npm-unknown' }
    } elseif ($Executable -eq 'python') {
      $pipCategory = if ($errTask.Result -match '(?i)Temporary failure in name resolution|Name or service not known') { 'network-dns' }
        elseif ($errTask.Result -match '(?i)CERTIFICATE_VERIFY_FAILED|certificate verify failed') { 'tls-certificate' }
        elseif ($errTask.Result -match '(?i)Read timed out|timed out') { 'network-timeout' }
        elseif ($errTask.Result -match '(?i)Connection reset|Connection refused') { 'network-connection' }
        elseif ($errTask.Result -match '(?i)No matching distribution|Could not find a version') { 'package-not-found' }
        elseif ($errTask.Result -match '(?i)Access is denied|Permission denied') { 'permission' }
        elseif ($errTask.Result -match '(?i)No module named pip') { 'pip-missing' }
        else { 'pip-unknown' }
    }
    Set-InstallDiagnostic $Subphase $Executable $process.ExitCode $null $null $null $npmCode $pipCategory 'exit-nonzero'
    throw 'native installer failed'
  }
}
function Quote-CmdArgument([string]$Value) { return '"' + $Value.Replace('"', '\"') + '"' }
function Invoke-Download([string]$Uri, [string]$Destination) {
  $ProgressPreference = 'SilentlyContinue'
  try { Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec 180 -OutFile $Destination }
  catch {
    $status = $null
    try { $status = [int]$_.Exception.Response.StatusCode } catch { }
    Set-InstallDiagnostic 'download' 'http-download' $null $null $status $null
    throw
  }
  if (-not (Test-Path -LiteralPath $Destination) -or (Get-Item -LiteralPath $Destination).Length -eq 0) {
    Set-InstallDiagnostic 'download' 'http-download' $null $null $null 'empty-download'
    throw 'official download was empty'
  }
}
function Npm([string]$Package) {
  # npm.cmd is a shell shim. Resolve its adjacent npm-cli.js and invoke the
  # verified node.exe directly, preserving ArgumentList boundaries and the
  # isolated prefix/cache environment without cmd.exe serialization.
  $node = (Get-Command node.exe -ErrorAction Stop).Source
  $npmShim = (Get-Command npm.cmd -ErrorAction Stop).Source
  $npmCli = Join-Path (Split-Path -Parent $npmShim) 'node_modules/npm/bin/npm-cli.js'
  if (-not (Test-Path -LiteralPath $npmCli -PathType Leaf)) {
    Set-InstallDiagnostic 'install' 'npm-node' $null $null $null 'expected-executable-missing'
    throw 'npm cli was not found beside npm.cmd'
  }
  Invoke-Native $node @($npmCli,'install','--global','--no-fund','--no-audit',$Package) 'npm-node' 'install'
}
function Invoke-OfficialScript([string]$Uri, [string[]]$Arguments) {
  $script = Join-Path $tmp 'official-installer.ps1'; Invoke-Download $Uri $script
  $all = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$script) + $Arguments
  Invoke-Native 'pwsh.exe' $all 'pwsh' 'install'
}
function Invoke-HermesPinned([string]$Uri, [string[]]$Arguments) {
  $savedPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  $savedHome = [Environment]::GetEnvironmentVariable('HERMES_HOME', 'User')
  $savedGit = [Environment]::GetEnvironmentVariable('HERMES_GIT_BASH_PATH', 'User')
  try { Invoke-OfficialScript $Uri $Arguments }
  finally {
    [Environment]::SetEnvironmentVariable('Path', $savedPath, 'User')
    [Environment]::SetEnvironmentVariable('HERMES_HOME', $savedHome, 'User')
    [Environment]::SetEnvironmentVariable('HERMES_GIT_BASH_PATH', $savedGit, 'User')
  }
}
function GitHubReleaseAssets([string]$Repository, [string]$Tag) {
  $headers = @{ 'User-Agent' = 'nan-harness-windows-canary'; 'Accept' = 'application/vnd.github+json' }
  $api = "https://api.github.com/repos/$Repository/releases/tags/$Tag"
  try { $release = Invoke-RestMethod -Uri $api -Headers $headers -TimeoutSec 30 }
  catch {
    $status = $null
    try { $status = [int]$_.Exception.Response.StatusCode } catch { }
    Set-InstallDiagnostic 'metadata' 'github-api' $null $null $status $null
    throw
  }
  if (-not $release.assets) {
    Set-InstallDiagnostic 'asset-selection' 'github-api' $null $null $null 'release-empty'
    throw 'official release has no assets'
  }
  return @($release.assets)
}
function Install-ArchiveAsset([string]$Uri, [string]$Pattern, [string]$OutputName) {
  $archive = Join-Path $tmp 'asset.zip'; Invoke-Download $Uri $archive
  try { Expand-Archive -LiteralPath $archive -DestinationPath $tmp -Force }
  catch {
    Set-InstallDiagnostic 'archive' 'archive-extract' $null $null $null 'invalid-archive'
    throw
  }
  $candidate = Get-ChildItem -LiteralPath $tmp -Recurse -File | Where-Object { $_.Name -match $Pattern } | Select-Object -First 1
  if (-not $candidate) {
    Set-InstallDiagnostic 'asset-selection' 'archive' $null $null $null 'expected-executable-missing'
    throw 'official archive did not contain its expected executable'
  }
  Copy-Item -LiteralPath $candidate.FullName -Destination (Join-Path $bin $OutputName) -Force
  foreach ($dll in Get-ChildItem -LiteralPath $tmp -Recurse -File -Filter '*.dll') {
    Copy-Item -LiteralPath $dll.FullName -Destination (Join-Path $bin $dll.Name) -Force
  }
}
function Probe-OfficialWindowsMetadata([string]$Uri, [string]$Name) {
  $ProgressPreference = 'SilentlyContinue'
  try {
    $response = Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec 30
    $text = [string]$response.Content
    if ($text -match '(?i)windows|win32|windows-x64|pc-windows') {
      Set-InstallDiagnostic 'metadata' 'official-metadata' $null $null ([int]$response.StatusCode) 'windows-mapping-missing'
      throw "$Name declared a Windows platform but no verified installer mapping exists"
    }
    Set-InstallDiagnostic 'metadata' 'official-metadata' $null $null ([int]$response.StatusCode) 'metadata-inconclusive'
    throw "$Name official platform metadata is inconclusive for Windows"
  } catch [System.Net.WebException] {
    $status = $null
    try { $status = [int]$_.Exception.Response.StatusCode } catch { }
    Set-InstallDiagnostic 'metadata' 'official-metadata' $null $null $status 'metadata-request-failed'
    throw "$Name official platform metadata probe failed"
  }
}
try {
  switch ($Harness) {
    'claude-code' { Npm "@anthropic-ai/claude-code@$Version" }
    'codex' { Npm "@openai/codex@$Version" }
    'opencode' { Npm "opencode-ai@$Version" }
    'pi' { Npm "@earendil-works/pi-coding-agent@$Version" }
    'deepseek-harness' { Npm "@deepseek-ai/dsh@$Version" }
    'openclaw' {
      # The official installer matches the product's recipe and bootstraps the portable Git
      # the harness's shell tools need; version-doctor verifies the installed version.
      Invoke-OfficialScript 'https://openclaw.ai/install.ps1' @('-NoOnboard')
    }
    'cline' { Npm "cline@$Version" }
    'qwen-code' { Npm "@qwen-code/qwen-code@$Version" }
    'hermes' {
      if ($Ref -notmatch '^[0-9a-f]{40}$') {
        Set-InstallDiagnostic 'metadata' 'official-metadata' $null $null $null 'invalid-ref'
        throw 'hermes requires an immutable source ref'
      }
      $hermesHome = Join-Path $cell 'hermes'; $hermesInstall = Join-Path $hermesHome 'hermes-agent'
      Invoke-HermesPinned "https://raw.githubusercontent.com/NousResearch/hermes-agent/$Ref/scripts/install.ps1" @('-SkipSetup','-NoVenv','-HermesHome',$hermesHome,'-InstallDir',$hermesInstall,'-Commit',$Ref,'-ForceCommit')
      # The product resolves `hermes` from PATH, and the installer's own directory is not on
      # the cell's PATH, so a shim that forwards to the installed launcher is written where
      # the cell already looks.
      $launcher = Get-ChildItem -LiteralPath $hermesInstall -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -in @('hermes.exe','hermes.cmd','hermes.bat','hermes') } | Select-Object -First 1
      if (-not $launcher) {
        Set-InstallDiagnostic 'asset-selection' 'official-installer' $null $null $null 'expected-executable-missing'
        throw 'hermes installer did not produce a launcher'
      }
      $shim = '@echo off' + [Environment]::NewLine + 'call "' + $launcher.FullName + '" %*' + [Environment]::NewLine
      [IO.File]::WriteAllText((Join-Path $bin 'hermes.cmd'), $shim, [Text.UTF8Encoding]::new($false))
    }
    'omp' {
      $assets = GitHubReleaseAssets 'can1357/oh-my-pi' "v$Version"
      $asset = $assets | Where-Object { $_.name -eq 'omp-windows-x64.exe' } | Select-Object -First 1
      if (-not $asset) {
        Set-InstallDiagnostic 'asset-selection' 'github-api' $null $null $null 'expected-asset-missing'
        throw 'official OMP release has no Windows x64 asset'
      }
      Invoke-Download $asset.browser_download_url (Join-Path $bin 'omp.exe')
    }
    'kimi-code' {
      # The product installs the vendor's own Kimi CLI on both platforms; the cell pins the
      # resolved version and keeps the install inside its private home.
      $installer = Join-Path $tmp 'kimi-install.ps1'
      Invoke-Download 'https://code.kimi.com/kimi-code/install.ps1' $installer
      $env:KIMI_VERSION = $Version
      $env:KIMI_INSTALL_DIR = Join-Path $env:USERPROFILE '.kimi-code'
      try { Invoke-Native 'pwsh.exe' @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$installer) 'pwsh' 'install' }
      finally { Remove-Item Env:KIMI_VERSION -ErrorAction SilentlyContinue }
    }
    'goose' {
      if ($Version -notmatch '^[0-9A-Za-z][0-9A-Za-z.-]*$') {
        Set-InstallDiagnostic 'metadata' 'official-metadata' $null $null $null 'invalid-version'
        throw 'goose version is not a closed release identifier'
      }
      $assets = GitHubReleaseAssets 'aaif-goose/goose' "v$Version"
      $asset = $assets | Where-Object { $_.name -eq 'goose-x86_64-pc-windows-msvc.zip' } | Select-Object -First 1
      if (-not $asset) {
        Set-InstallDiagnostic 'asset-selection' 'github-api' $null $null $null 'expected-asset-missing'
        throw 'official Goose release has no Windows x64 asset'
      }
      Install-ArchiveAsset $asset.browser_download_url '(^|[\\/])goose\.exe$' 'goose.exe'
    }
    'aider' {
      $venv = Join-Path $env:USERPROFILE '.nan-harness-canary-venv'
      Invoke-Native 'py.exe' @("-$PythonVersion",'-m','venv',$venv) 'py-launcher' 'virtualenv'
      Invoke-Native (Join-Path $venv 'Scripts/python.exe') @('-m','pip','install',"aider-chat==$Version") 'python' 'install'
      Copy-Item (Join-Path $venv 'Scripts/aider.exe') (Join-Path $bin 'aider.exe') -Force
    }
    'prime-agent' { Probe-OfficialWindowsMetadata 'https://pub-728493de92a943e2a9b2d17b4719f318.r2.dev/stable' 'prime-agent' }
    'fx' { Probe-OfficialWindowsMetadata 'https://releases.fx.sh/latest.txt' 'fx' }
  }
  Write-InstallerResult 'passed' 'passed'
} catch {
  $message = [string]$_.Exception.Message
  $reason = if ($message -match 'capability-not-implemented') { 'capability-not-implemented' }
           elseif ($message -match 'official (?:OMP|Kimi|Goose) release has no|archive did not contain') { 'official-asset-missing' }
           elseif ($message -match 'official platform metadata is inconclusive') { 'official-metadata-probe-failed' }
           elseif ($message -match 'platform metadata probe failed') { 'official-metadata-probe-failed' }
           elseif ($message -match 'requires an immutable') { 'invalid-frozen-ref' }
           elseif ($message -match 'version') { 'invalid-version' }
           else { 'installer-failed' }
  Write-InstallerResult 'failed' $reason
  exit 1
} finally {
  Remove-Item -LiteralPath $stdoutLog,$stderrLog -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
