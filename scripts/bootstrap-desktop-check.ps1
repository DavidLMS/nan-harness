# Run with & ([scriptblock]::Create((Invoke-RestMethod <official script URL>))) @arguments.
# The argument array is passed unchanged; no user command or PowerShell source is evaluated.
$ErrorActionPreference = 'Stop'
$checkerArguments = @($args)
$retainChecker = $checkerArguments -contains '--ephemeral'
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -ne 'X64') {
    throw 'The checker publishes Windows binaries for x64 only.'
}

function Get-CheckerDownload {
    param([string]$Url, [string]$Destination, [long]$MaximumBytes)
    Add-Type -AssemblyName System.Net.Http
    $handler = [Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $false
    $handler.UseCookies = $false
    $client = [Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromMinutes(5)
    $cancellation = [Threading.CancellationTokenSource]::new([TimeSpan]::FromMinutes(5))
    $response = $null
    try {
        $uri = [Uri]$Url
        for ($redirect = 0; $redirect -le 8; $redirect++) {
            if ($uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.Fragment) {
                throw 'Unsafe download address.'
            }
            $response = $client.GetAsync($uri, [Net.Http.HttpCompletionOption]::ResponseHeadersRead, $cancellation.Token).GetAwaiter().GetResult()
            if ([int]$response.StatusCode -ge 300 -and [int]$response.StatusCode -lt 400) {
                if ($redirect -eq 8 -or -not $response.Headers.Location) { throw 'Invalid redirect.' }
                $uri = [Uri]::new($uri, $response.Headers.Location)
                $response.Dispose()
                $response = $null
                continue
            }
            if (-not $response.IsSuccessStatusCode -or $response.Content.Headers.ContentLength -gt $MaximumBytes) {
                throw 'Download status or size is invalid.'
            }
            $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
            $outputStream = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            try {
                $buffer = [byte[]]::new(65536)
                $received = 0L
                while (($count = $inputStream.ReadAsync($buffer, 0, $buffer.Length, $cancellation.Token).GetAwaiter().GetResult()) -gt 0) {
                    $received += $count
                    if ($received -gt $MaximumBytes) { throw 'Download exceeds limit.' }
                    $outputStream.Write($buffer, 0, $count)
                }
                if ($received -eq 0) { throw 'Empty download.' }
                $outputStream.Flush($true)
            } finally {
                $outputStream.Dispose()
                $inputStream.Dispose()
            }
            return
        }
        throw 'Download did not complete.'
    } catch {
        throw 'The checker download failed. Check your connection and retry. No installed applications were changed.'
    } finally {
        if ($response) { $response.Dispose() }
        $cancellation.Dispose()
        $client.Dispose()
        $handler.Dispose()
    }
}

$checkerDirectory = Join-Path ([IO.Path]::GetTempPath()) "nanh-desktop-check-$([Guid]::NewGuid().ToString('N'))"
if (Test-Path -LiteralPath $checkerDirectory) {
    throw 'The private checker directory already exists; retry the command.'
}
[IO.Directory]::CreateDirectory($checkerDirectory) | Out-Null
$checkerExit = 1
try {
    # Apply the private directory boundary before downloading even the first byte.
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent().User
    $acl = Get-Acl -LiteralPath $checkerDirectory
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($existingRule in @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))) {
        $acl.PurgeAccessRules($existingRule.IdentityReference)
    }
    $acl.SetOwner($identity)
    foreach ($principal in @($identity, [Security.Principal.SecurityIdentifier]::new('S-1-5-18'))) {
        $rule = [Security.AccessControl.FileSystemAccessRule]::new($principal, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
    }
    Set-Acl -LiteralPath $checkerDirectory -AclObject $acl
    $verifiedAcl = Get-Acl -LiteralPath $checkerDirectory
    $allowedPrincipals = @($identity.Value, 'S-1-5-18') | Select-Object -Unique
    $verifiedRules = @($verifiedAcl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    if (-not $verifiedAcl.AreAccessRulesProtected -or
        $verifiedAcl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $identity.Value -or
        $verifiedRules.Count -ne $allowedPrincipals.Count) {
        throw 'The checker directory could not be made private; no files were downloaded.'
    }
    foreach ($rule in $verifiedRules) {
        if ($rule.IdentityReference.Value -notin $allowedPrincipals -or $rule.IsInherited -or
            $rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow -or
            $rule.FileSystemRights -ne [Security.AccessControl.FileSystemRights]::FullControl -or
            $rule.InheritanceFlags -ne [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit' -or
            $rule.PropagationFlags -ne [Security.AccessControl.PropagationFlags]::None) {
            throw 'The checker directory grants unexpected access; no files were downloaded.'
        }
    }

    $repositoryUrl = 'https://github.com/DavidLMS/nan-harness/releases/download'
    if ($env:NAN_DESKTOP_CHECK_VERSION) {
        $checkerVersion = $env:NAN_DESKTOP_CHECK_VERSION
    } else {
        $versionFile = Join-Path $checkerDirectory 'release-version.txt'
        Get-CheckerDownload "$repositoryUrl/desktop-check/release-version.txt" $versionFile 128
        $checkerVersion = [IO.File]::ReadAllText($versionFile).Trim()
    }
    if ($checkerVersion -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
        throw 'The checker channel returned an invalid version.'
    }
    $baseUrl = "$repositoryUrl/desktop-check-v$checkerVersion"
    $artifact = 'nanh-desktop-check-x86_64-pc-windows-msvc.exe'
    $candidate = Join-Path $checkerDirectory $artifact
    $checksumFile = Join-Path $checkerDirectory 'SHA256SUMS'
    Get-CheckerDownload "$baseUrl/SHA256SUMS" $checksumFile 65536
    Get-CheckerDownload "$baseUrl/$artifact" $candidate 268435456
    $matchesFound = @([IO.File]::ReadAllLines($checksumFile) | Where-Object { ($_ -split '\s+').Count -eq 2 -and ($_ -split '\s+')[1] -ceq $artifact })
    if ($matchesFound.Count -ne 1) { throw 'The checksum manifest does not identify exactly one checker binary.' }
    $expected = ($matchesFound[0] -split '\s+')[0]
    if ($expected -notmatch '^[0-9a-fA-F]{64}$' -or (Get-FileHash -Algorithm SHA256 -LiteralPath $candidate).Hash -ine $expected) {
        throw 'The downloaded checker failed SHA-256 verification.'
    }
    $reportedVersion = (& $candidate --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $reportedVersion -cne "nanh-desktop-check $checkerVersion") {
        throw 'The downloaded checker reports an unexpected version.'
    }
    & $candidate @checkerArguments
    $checkerExit = $LASTEXITCODE
} finally {
    if ($retainChecker) {
        Write-Host "Retained checker download in $checkerDirectory"
    } else {
        Remove-Item -LiteralPath $checkerDirectory -Recurse -Force
    }
}
$global:LASTEXITCODE = $checkerExit
if ($checkerExit -ne 0) {
    throw "Desktop checker did not complete successfully (exit status $checkerExit)."
}
