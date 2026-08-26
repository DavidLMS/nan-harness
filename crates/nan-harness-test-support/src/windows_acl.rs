use serde::Deserialize;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::process::Command;

const SYSTEM_SID: &str = "S-1-5-18";
const POWERSHELL_SCRIPT: &str = r"
$path = $args[0]
$acl = Get-Acl -LiteralPath $path
$currentUserSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$rules = @(
    foreach ($rule in @($acl.Access)) {
        [pscustomobject]@{
            Sid = $rule.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
            AccessControlType = $rule.AccessControlType.ToString()
            FileSystemRights = $rule.FileSystemRights.ToString()
            InheritanceFlags = $rule.InheritanceFlags.ToString()
            PropagationFlags = $rule.PropagationFlags.ToString()
            IsInherited = [bool]$rule.IsInherited
        }
    }
)
[pscustomobject]@{
    CurrentUserSid = $currentUserSid
    Protected = [bool]$acl.AreAccessRulesProtected
    Rules = $rules
} | ConvertTo-Json -Compress -Depth 5
";

#[derive(Debug, Deserialize)]
struct AclReport {
    #[serde(rename = "CurrentUserSid")]
    current_user_sid: String,
    #[serde(rename = "Protected")]
    protected: bool,
    #[serde(rename = "Rules")]
    rules: Vec<AclRule>,
}

#[derive(Debug, Deserialize)]
struct AclRule {
    #[serde(rename = "Sid")]
    sid: String,
    #[serde(rename = "AccessControlType")]
    access_control_type: String,
    #[serde(rename = "FileSystemRights")]
    file_system_rights: String,
    #[serde(rename = "InheritanceFlags")]
    inheritance_flags: String,
    #[serde(rename = "PropagationFlags")]
    propagation_flags: String,
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
        return Err(io::Error::other("PowerShell ACL inspection failed"));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| io::Error::other(format!("invalid PowerShell ACL report: {error}")))
}

fn assert_allowed_rules(report: &AclReport, protected: bool) -> io::Result<()> {
    if report.protected != protected {
        return Err(io::Error::other("unexpected protected-DACL state"));
    }
    if report.rules.len() != 2 {
        return Err(io::Error::other(
            "private DACL contains an unexpected ACE count",
        ));
    }

    let expected = HashSet::from([report.current_user_sid.clone(), SYSTEM_SID.to_owned()]);
    let mut actual = HashSet::new();
    for rule in &report.rules {
        if rule.access_control_type != "Allow" || rule.file_system_rights != "FullControl" {
            return Err(io::Error::other(
                "private DACL contains an unexpected access rule",
            ));
        }
        if rule.propagation_flags != "None" {
            return Err(io::Error::other(
                "private DACL contains unexpected propagation flags",
            ));
        }
        if !actual.insert(rule.sid.clone()) {
            return Err(io::Error::other("private DACL contains a duplicate SID"));
        }
    }
    if actual != expected {
        return Err(io::Error::other(
            "private DACL is not limited to the current user and SYSTEM",
        ));
    }
    Ok(())
}

fn has_inheritance_flags(rule: &AclRule, expected: &[&str]) -> bool {
    let actual = rule
        .inheritance_flags
        .split(',')
        .map(str::trim)
        .collect::<HashSet<_>>();
    let expected = expected.iter().copied().collect::<HashSet<_>>();
    actual == expected
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
        .any(|rule| !has_inheritance_flags(rule, &["None"]) || rule.is_inherited)
    {
        return Err(io::Error::other(
            "private file DACL contains inheritance metadata",
        ));
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
    if report.rules.iter().any(|rule| {
        !has_inheritance_flags(rule, &["ContainerInherit", "ObjectInherit"]) || rule.is_inherited
    }) {
        return Err(io::Error::other(
            "private directory DACL does not propagate exactly to objects and containers",
        ));
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
        return Err(io::Error::other(
            "private descendant DACL is not inherited from its protected parent",
        ));
    }
    Ok(())
}
