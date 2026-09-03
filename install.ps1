$ErrorActionPreference = "Stop"

function Stop-Install {
    param([string]$Message)
    throw "nan-harness installation failed: $Message"
}

$repository = if ($env:NAN_INSTALL_REPOSITORY) {
    $env:NAN_INSTALL_REPOSITORY
} else {
    "DavidLMS/nan-harness"
}
if ($repository -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') {
    Stop-Install "NAN_INSTALL_REPOSITORY must use the owner/name format"
}

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
if ($architecture -ne "X64") {
    Stop-Install "nan-harness does not publish a Windows binary for $architecture"
}
$target = "x86_64-pc-windows-msvc"
$artifact = "nan-harness-$target.exe"
$baseUrl = if ($env:NAN_INSTALL_BASE_URL) {
    $env:NAN_INSTALL_BASE_URL.TrimEnd('/')
} else {
    "https://github.com/$repository/releases/latest/download"
}
$baseUri = $null
if (-not [Uri]::TryCreate($baseUrl, [UriKind]::Absolute, [ref]$baseUri)) {
    Stop-Install "NAN_INSTALL_BASE_URL is invalid"
}
if ($baseUri.Scheme -ne "https" -and -not $baseUri.IsLoopback) {
    Stop-Install "NAN_INSTALL_BASE_URL must use HTTPS"
}

$temporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) "nan-install-$([Guid]::NewGuid().ToString('N'))"
[IO.Directory]::CreateDirectory($temporaryDirectory) | Out-Null
try {
    $candidate = Join-Path $temporaryDirectory $artifact
    $checksumFile = "$candidate.sha256"
    $versionFile = Join-Path $temporaryDirectory "release-version.txt"
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$artifact" -OutFile $candidate
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$artifact.sha256" -OutFile $checksumFile
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/release-version.txt" -OutFile $versionFile

    $checksumParts = (Get-Content -Raw $checksumFile).Trim() -split '\s+'
    $expectedChecksum = $checksumParts[0]
    if ($expectedChecksum -notmatch '^[0-9A-Fa-f]{64}$') {
        Stop-Install "the release checksum is invalid"
    }
    $candidateStream = [IO.File]::OpenRead($candidate)
    try {
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try {
            $hashBytes = $sha256.ComputeHash($candidateStream)
            $actualChecksum = [BitConverter]::ToString($hashBytes).Replace("-", "")
        } finally {
            $sha256.Dispose()
        }
    } finally {
        $candidateStream.Dispose()
    }
    if ($actualChecksum -ne $expectedChecksum) {
        Stop-Install "the downloaded binary failed SHA-256 verification"
    }

    $releaseVersion = (Get-Content -Raw $versionFile).Trim()
    if ($releaseVersion -notmatch '^[0-9A-Za-z.+-]+$') {
        Stop-Install "the release version is invalid"
    }
    $candidateVersion = (& $candidate --version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        Stop-Install "the downloaded binary did not pass its version check"
    }
    if ($candidateVersion -cne "nan-harness $releaseVersion") {
        Stop-Install "the downloaded binary does not report version $releaseVersion"
    }

    $installDirectory = if ($env:NAN_INSTALL_DIR) {
        $env:NAN_INSTALL_DIR
    } elseif ($env:LOCALAPPDATA) {
        Join-Path $env:LOCALAPPDATA "Programs\nan-harness\bin"
    } else {
        Join-Path $HOME ".local\bin"
    }
    [IO.Directory]::CreateDirectory($installDirectory) | Out-Null
    $destination = Join-Path $installDirectory "nan-harness.exe"
    if (Test-Path -PathType Container $destination) {
        Stop-Install "$destination is a directory"
    }

    foreach ($commandName in @("nanh", "nanh.com", "nanh.exe", "nanh.bat", "nanh.ps1")) {
        $commandPath = Join-Path $installDirectory $commandName
        if (Test-Path -LiteralPath $commandPath) {
            Stop-Install "$commandPath exists and is not the nan-harness command alias"
        }
    }

    $aliasPath = Join-Path $installDirectory "nanh.cmd"
    if (Test-Path -PathType Container $aliasPath) {
        Stop-Install "$aliasPath is a directory"
    }
    $aliasContents = "@echo off`r`n`"%~dp0nan-harness.exe`" %*`r`n"
    if (Test-Path -LiteralPath $aliasPath -PathType Leaf) {
        $existingAlias = [IO.File]::ReadAllText($aliasPath)
        if ($existingAlias -cne $aliasContents) {
            Stop-Install "$aliasPath exists and is not the nan-harness command alias"
        }
    }
    $stagedBinary = Join-Path $installDirectory ".nan-harness-$([Guid]::NewGuid().ToString('N')).exe"
    Copy-Item $candidate $stagedBinary
    Move-Item -Force $stagedBinary $destination

    $stagedAlias = Join-Path $installDirectory ".nanh-$([Guid]::NewGuid().ToString('N')).cmd"
    [IO.File]::WriteAllText($stagedAlias, $aliasContents, [Text.Encoding]::ASCII)
    Move-Item -Force $stagedAlias $aliasPath

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ';' | Where-Object { $_ })
    $pathEntryAdded = -not ($pathEntries | Where-Object { $_.TrimEnd('\') -ieq $installDirectory.TrimEnd('\') })
    if ($pathEntryAdded) {
        $updatedPath = (@($pathEntries) + $installDirectory) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $updatedPath, "User")
    }
    $env:Path = "$installDirectory;$env:Path"

    $receiptArguments = @(
        "__record-installation",
        "--executable", $destination,
        "--alias", $aliasPath
    )
    if ($pathEntryAdded) {
        $receiptArguments += "--user-path-entry-added"
    }
    & $destination @receiptArguments
    if ($LASTEXITCODE -ne 0) {
        Stop-Install "the installed binary could not record its installation"
    }

    $previousAliasPath = Join-Path $installDirectory "nan.cmd"
    if (Test-Path -LiteralPath $previousAliasPath -PathType Leaf) {
        $previousAliasContents = [IO.File]::ReadAllText($previousAliasPath)
        if ($previousAliasContents -ceq $aliasContents) {
            Remove-Item -LiteralPath $previousAliasPath -Force
        }
    }

    Write-Host "nan-harness $releaseVersion installed successfully in $installDirectory."
    Write-Host "Open a new terminal, then run nanh."
} finally {
    Remove-Item -Recurse -Force $temporaryDirectory -ErrorAction SilentlyContinue
}
