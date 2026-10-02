# Closed, read-only native proof. Never emits process names, paths or command lines.
param(
    [ValidateSet('endpoint', 'descendant')][string]$Mode,
    [ValidateRange(2, 2147483647)][int]$Value,
    [ValidateRange(2, 2147483647)][int]$Owner
)
$ErrorActionPreference = 'Stop'
function Test-Descendant([int]$Candidate, [int]$Root) {
    # One metadata snapshot avoids repeated CIM startup on every ancestor. Each
    # proof is fresh; never retain a PID map across renderer actions.
    $records = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CreationDate, SessionId)
    if ($records.Count -gt 4096) { return $false }
    $processes = @{}
    foreach ($record in $records) { $processes[[int]$record.ProcessId] = $record }
    $seen = [System.Collections.Generic.HashSet[int]]::new()
    for ($depth = 0; $depth -lt 32 -and $Candidate -gt 1; $depth++) {
        if (!$seen.Add($Candidate)) { return $false }
        $current = $processes[$Candidate]
        if (!$current -or !$current.CreationDate) { return $false }
        if ($Candidate -eq $Root) { return $true }
        $parent = $processes[[int]$current.ParentProcessId]
        # Microsoft documents PID reuse: a replacement parent can be younger.
        if (!$parent -or !$parent.CreationDate -or $parent.CreationDate -gt $current.CreationDate
            -or $parent.SessionId -ne $current.SessionId) { return $false }
        $Candidate = [int]$parent.ProcessId
    }
    return $false
}
try {
    if ($Mode -eq 'descendant') {
        $owned = Test-Descendant $Value $Owner
    } else {
        if ($Value -gt 65535) { throw 'invalid port' }
        $listeners = @(Get-NetTCPConnection -State Listen -LocalPort $Value -ErrorAction Stop)
        $owned = $listeners.Count -eq 1 -and $listeners[0].LocalAddress -eq '127.0.0.1' -and
            (Test-Descendant ([int]$listeners[0].OwningProcess) $Owner)
    }
    [Console]::Out.Write($(if ($owned) { 'true' } else { 'false' }))
} catch {
    [Console]::Out.Write('false')
    exit 1
}
