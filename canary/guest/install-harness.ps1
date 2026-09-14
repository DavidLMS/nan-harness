[CmdletBinding()]
param(
  [Parameter(Mandatory=$true)][ValidateSet('claude-code','codex','opencode','hermes','pi','omp','prime-agent','deepseek-harness','openclaw','cline','qwen-code','kimi-code','aider','goose','fx')][string]$Harness,
  [Parameter(Mandatory=$true)][string]$Version,
  [string]$Ref = ''
)
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion.Major -lt 7) {
  $pwsh = Get-Command pwsh.exe -ErrorAction Stop
  $forward = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$PSCommandPath,
               '-Harness',$Harness,'-Version',$Version)
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
function Write-InstallerResult([string]$Status, [string]$Reason) {
  $allowed = @('passed','installer-failed','official-asset-missing','official-metadata-probe-failed',
               'capability-not-implemented','invalid-frozen-ref','invalid-version')
  if ($allowed -notcontains $Reason) { $Reason = 'installer-failed' }
  $value = @{ schemaVersion = 1; status = $Status; reason = $Reason } | ConvertTo-Json -Compress
  [IO.File]::WriteAllText($resultPath, $value, [Text.UTF8Encoding]::new($false))
}

function Invoke-Native([string]$File, [string[]]$Arguments) {
  # ProcessStartInfo.ArgumentList preserves spaces and quotes without cmd.exe.
  $info = [System.Diagnostics.ProcessStartInfo]::new()
  $info.FileName = $File; $info.WorkingDirectory = $cell; $info.UseShellExecute = $false
  $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
  foreach ($argument in $Arguments) { [void]$info.ArgumentList.Add($argument) }
  $process = [System.Diagnostics.Process]::new(); $process.StartInfo = $info
  if (-not $process.Start()) { throw 'native installer process could not start' }
  $outTask = $process.StandardOutput.ReadToEndAsync(); $errTask = $process.StandardError.ReadToEndAsync()
  $process.WaitForExit()
  [IO.File]::WriteAllText($stdoutLog, $outTask.Result, [Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText($stderrLog, $errTask.Result, [Text.UTF8Encoding]::new($false))
  if ($process.ExitCode -ne 0) { throw 'native installer failed' }
}
function Invoke-Download([string]$Uri, [string]$Destination) {
  $ProgressPreference = 'SilentlyContinue'
  Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec 180 -OutFile $Destination
  if (-not (Test-Path -LiteralPath $Destination) -or (Get-Item -LiteralPath $Destination).Length -eq 0) { throw 'official download was empty' }
}
function Npm([string]$Package) {
  Invoke-Native 'npm.cmd' @('install','--global','--no-fund','--no-audit',$Package)
}
function Invoke-OfficialScript([string]$Uri, [string[]]$Arguments) {
  $script = Join-Path $tmp 'official-installer.ps1'; Invoke-Download $Uri $script
  $all = @('-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$script) + $Arguments
  Invoke-Native 'pwsh.exe' $all
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
  $release = Invoke-RestMethod -Uri $api -Headers $headers -TimeoutSec 30
  if (-not $release.assets) { throw 'official release has no assets' }
  return @($release.assets)
}
function Install-ArchiveAsset([string]$Uri, [string]$Pattern, [string]$OutputName) {
  $archive = Join-Path $tmp 'asset.zip'; Invoke-Download $Uri $archive
  Expand-Archive -LiteralPath $archive -DestinationPath $tmp -Force
  $candidate = Get-ChildItem -LiteralPath $tmp -Recurse -File | Where-Object { $_.Name -match $Pattern } | Select-Object -First 1
  if (-not $candidate) { throw 'official archive did not contain its expected executable' }
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
      throw "$Name declared a Windows platform but no verified installer mapping exists"
    }
    throw "$Name official platform metadata has no Windows asset"
  } catch [System.Net.WebException] {
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
    'openclaw' { Npm "openclaw@$Version" }
    'cline' { Npm "cline@$Version" }
    'qwen-code' { Npm "@qwen-code/qwen-code@$Version" }
    'hermes' {
      if (-not $Ref -match '^[0-9a-f]{40}$') { throw 'hermes requires an immutable source ref' }
      $hermesHome = Join-Path $cell 'hermes'; $hermesInstall = Join-Path $hermesHome 'hermes-agent'
      Invoke-HermesPinned "https://raw.githubusercontent.com/NousResearch/hermes-agent/$Ref/scripts/install.ps1" @('-SkipSetup','-NoVenv','-HermesHome',$hermesHome,'-InstallDir',$hermesInstall,'-Commit',$Ref,'-ForceCommit')
    }
    'omp' {
      $assets = GitHubReleaseAssets 'can1357/oh-my-pi' "v$Version"
      $asset = $assets | Where-Object { $_.name -eq 'omp-windows-x64.exe' } | Select-Object -First 1
      if (-not $asset) { throw 'official OMP release has no Windows x64 asset' }
      Invoke-Download $asset.browser_download_url (Join-Path $bin 'omp.exe')
    }
    'kimi-code' {
      $assets = GitHubReleaseAssets 'MoonshotAI/kimi-cli' $Version
      $asset = $assets | Where-Object { $_.name -match 'x86_64-pc-windows-msvc\.zip$' } | Select-Object -First 1
      if (-not $asset) { throw 'official Kimi release has no Windows x64 archive' }
      Install-ArchiveAsset $asset.browser_download_url '(^|[\\/])kimi(\.exe)?$' 'kimi.exe'
    }
    'goose' {
      if (-not $Version -match '^[0-9A-Za-z][0-9A-Za-z.-]*$') { throw 'goose version is not a closed release identifier' }
      $assets = GitHubReleaseAssets 'aaif-goose/goose' "v$Version"
      $asset = $assets | Where-Object { $_.name -eq 'goose-x86_64-pc-windows-msvc.zip' } | Select-Object -First 1
      if (-not $asset) { throw 'official Goose release has no Windows x64 asset' }
      Install-ArchiveAsset $asset.browser_download_url '(^|[\\/])goose\.exe$' 'goose.exe'
    }
    'aider' {
      $venv = Join-Path $env:USERPROFILE '.nan-harness-canary-venv'
      Invoke-Native 'py.exe' @('-m','venv',$venv)
      Invoke-Native (Join-Path $venv 'Scripts/python.exe') @('-m','pip','install',"aider-chat==$Version")
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
