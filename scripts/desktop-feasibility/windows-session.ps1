# Read-only hosted desktop preflight. Never emit window names or COM error text.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') { exit 1 }
$facts = [ordered]@{ schemaVersion = 1; mechanism = 'windows-session'; diagnosticsOnly = $true;
  userInteractive = [Environment]::UserInteractive; sameConsoleSession = $false;
  foregroundPresent = $false; displayCount = $null; uiaRootPresent = $false;
  uiaChildCount = $null; errorCategory = $null; errorCode = $null }
try {
  Add-Type -AssemblyName System.Windows.Forms
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class HostedDesktop {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
}
'@
  $facts.sameConsoleSession = [Diagnostics.Process]::GetCurrentProcess().SessionId -eq [HostedDesktop]::WTSGetActiveConsoleSessionId()
  $facts.foregroundPresent = [HostedDesktop]::GetForegroundWindow() -ne [IntPtr]::Zero
  $count = [System.Windows.Forms.Screen]::AllScreens.Count
  if ($count -gt 32) { throw 'Bound' }
  $facts.displayCount = $count
} catch {
  $facts.errorCategory = 'native-query'
  $facts.errorCode = $_.Exception.HResult
}
try {
  Add-Type -AssemblyName UIAutomationClient
  Add-Type -AssemblyName UIAutomationTypes
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $facts.uiaRootPresent = $null -ne $root
  if ($null -ne $root) {
    $children = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
    if ($children.Count -gt 4096) { throw 'Bound' }
    $facts.uiaChildCount = $children.Count
  }
} catch {
  $facts.errorCategory = 'uia-query'
  $facts.errorCode = $_.Exception.HResult
}
[Console]::Out.Write(($facts | ConvertTo-Json -Compress))
