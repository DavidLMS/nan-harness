use serde::Deserialize;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::process::Command;

const SYSTEM_SID: &str = "S-1-5-18";
const ALLOW: i32 = 0;
const FULL_CONTROL: i64 = 2_032_127;
const NO_INHERITANCE: i32 = 0;
const OBJECT_AND_CONTAINER_INHERIT: i32 = 3;
const DACL_PROTECTED: i32 = 0x1000;
const POWERSHELL_SCRIPT: &str = r"
$path = $args[0]
$acl = Get-Acl -LiteralPath $path
$currentUserSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$sddl = $acl.GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::All)
$controlFlags = [int]([System.Security.AccessControl.RawSecurityDescriptor]::new($sddl).ControlFlags)
$rules = @(
    foreach ($rule in @($acl.Access)) {
        [pscustomobject]@{
            Sid = $rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
            AccessControlType = [int]$rule.AccessControlType
            FileSystemRights = [int64]$rule.FileSystemRights
            InheritanceFlags = [int]$rule.InheritanceFlags
            PropagationFlags = [int]$rule.PropagationFlags
            IsInherited = [bool]$rule.IsInherited
        }
    }
)
[pscustomobject]@{
    CurrentUserSid = $currentUserSid
    Protected = [bool]$acl.AreAccessRulesProtected
    Sddl = $sddl
    ControlFlags = $controlFlags
    DaclProtected = [bool]($controlFlags -band 0x1000)
    Rules = $rules
} | ConvertTo-Json -Compress -Depth 5
";

#[derive(Debug, Deserialize)]
struct AclReport {
    #[serde(rename = "CurrentUserSid")]
    current_user_sid: String,
    #[serde(rename = "Protected")]
    protected: bool,
    #[serde(rename = "Sddl")]
    sddl: String,
    #[serde(rename = "ControlFlags")]
    control_flags: i32,
    #[serde(rename = "DaclProtected")]
    dacl_protected: bool,
    #[serde(rename = "Rules")]
    rules: Vec<AclRule>,
}

#[derive(Debug, Deserialize)]
struct AclRule {
    #[serde(rename = "Sid")]
    sid: String,
    #[serde(rename = "AccessControlType")]
    access_control_type: i32,
    #[serde(rename = "FileSystemRights")]
    file_system_rights: i64,
    #[serde(rename = "InheritanceFlags")]
    inheritance_flags: i32,
    #[serde(rename = "PropagationFlags")]
    propagation_flags: i32,
    #[serde(rename = "IsInherited")]
    is_inherited: bool,
}

fn read_acl(path: &Path) -> io::Result<AclReport> {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            POWERSHELL_SCRIPT,
        ])
        .arg(path.as_os_str())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "PowerShell ACL inspection failed (status: {}): stdout: {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        io::Error::other(format!(
            "invalid PowerShell ACL report: {error}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        ))
    })
}

fn assert_allowed_rules(report: &AclReport, protected: bool) -> io::Result<()> {
    if report.protected != protected
        || report.dacl_protected != protected
        || report.dacl_protected != (report.control_flags & DACL_PROTECTED != 0)
    {
        return Err(io::Error::other(format!(
            "unexpected protected-DACL state (expected {protected}, got AreAccessRulesProtected={}, DaclProtected={}, ControlFlags={}, SDDL={}): {report:?}",
            report.protected, report.dacl_protected, report.control_flags, report.sddl
        )));
    }
    if report.rules.len() != 2 {
        return Err(io::Error::other(format!(
            "private DACL contains an unexpected ACE count (got {}): {report:?}",
            report.rules.len()
        )));
    }

    let expected = HashSet::from([report.current_user_sid.clone(), SYSTEM_SID.to_owned()]);
    let mut actual = HashSet::new();
    for rule in &report.rules {
        if rule.access_control_type != ALLOW || rule.file_system_rights != FULL_CONTROL {
            return Err(io::Error::other(format!(
                "private DACL contains an unexpected access rule: {report:?}"
            )));
        }
        if rule.propagation_flags != NO_INHERITANCE {
            return Err(io::Error::other(format!(
                "private DACL contains unexpected propagation flags: {report:?}"
            )));
        }
        if !actual.insert(rule.sid.clone()) {
            return Err(io::Error::other(format!(
                "private DACL contains a duplicate SID: {report:?}"
            )));
        }
    }
    if actual != expected {
        return Err(io::Error::other(format!(
            "private DACL is not limited to the current user and SYSTEM: {report:?}"
        )));
    }
    Ok(())
}

/// Assert that a file has a protected DACL with exactly owner and `SYSTEM`.
///
/// # Errors
///
/// Returns an error when PowerShell cannot inspect the path or the ACL differs
/// from the private-file contract.
pub fn assert_private_file(path: &Path) -> io::Result<()> {
    let report = read_acl(path)?;
    assert_allowed_rules(&report, true)?;
    if report
        .rules
        .iter()
        .any(|rule| rule.inheritance_flags != NO_INHERITANCE || rule.is_inherited)
    {
        return Err(io::Error::other(format!(
            "private file DACL contains inheritance metadata: {report:?}"
        )));
    }
    Ok(())
}

/// Assert that a directory has a protected owner-and-`SYSTEM` DACL that
/// propagates to objects and containers.
///
/// # Errors
///
/// Returns an error when PowerShell cannot inspect the path or the ACL differs
/// from the private-directory contract.
pub fn assert_private_directory(path: &Path) -> io::Result<()> {
    let report = read_acl(path)?;
    assert_allowed_rules(&report, true)?;
    if report
        .rules
        .iter()
        .any(|rule| rule.inheritance_flags != OBJECT_AND_CONTAINER_INHERIT || rule.is_inherited)
    {
        return Err(io::Error::other(format!(
            "private directory DACL does not propagate exactly to objects and containers: {report:?}"
        )));
    }
    Ok(())
}

/// Assert that an inherited descendant DACL contains only owner and `SYSTEM`.
///
/// # Errors
///
/// Returns an error when PowerShell cannot inspect the path or the ACL contains
/// any principal or rule outside the private-filesystem contract.
pub fn assert_private_descendant(path: &Path) -> io::Result<()> {
    let report = read_acl(path)?;
    assert_allowed_rules(&report, false)?;
    if report.rules.iter().any(|rule| !rule.is_inherited) {
        return Err(io::Error::other(format!(
            "private descendant DACL is not inherited from its protected parent: {report:?}"
        )));
    }
    Ok(())
}
