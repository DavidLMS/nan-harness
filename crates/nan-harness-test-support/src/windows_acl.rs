use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::process::Command;
use std::str;

const ACL_PATH_ENVIRONMENT_VARIABLE: &str = "NAN_HARNESS_TEST_ACL_PATH";
const EVERYONE_SID: &str = "S-1-1-0";
const SYSTEM_SID: &str = "S-1-5-18";
const ALLOW: i32 = 0;
const FULL_CONTROL: i64 = 2_032_127;
const NO_INHERITANCE: i32 = 0;
const OBJECT_AND_CONTAINER_INHERIT: i32 = 3;
const DACL_PROTECTED: i32 = 0x1000;

const POWERSHELL_READ_ACL_SCRIPT: &str = r"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$path = [System.Environment]::GetEnvironmentVariable('NAN_HARNESS_TEST_ACL_PATH')
if ([string]::IsNullOrWhiteSpace($path)) {
    throw 'NAN_HARNESS_TEST_ACL_PATH is required'
}

$attributes = [System.IO.File]::GetAttributes($path)
if (($attributes -band [System.IO.FileAttributes]::Directory) -ne 0) {
    $item = [System.IO.DirectoryInfo]::new($path)
    $acl = $item.GetAccessControl()
} else {
    $item = [System.IO.FileInfo]::new($path)
    $acl = $item.GetAccessControl()
}

$currentUserSid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$sddl = $acl.GetSecurityDescriptorSddlForm([System.Security.AccessControl.AccessControlSections]::All)
$controlFlags = [int]([System.Security.AccessControl.RawSecurityDescriptor]::new($sddl).ControlFlags)
[Console]::Out.WriteLine(('CURRENT_USER_SID|{0}' -f $currentUserSid))
[Console]::Out.WriteLine(('PROTECTED|{0}' -f [int]$acl.AreAccessRulesProtected))
[Console]::Out.WriteLine(('CONTROL_FLAGS|{0}' -f $controlFlags))
foreach ($rule in @($acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))) {
    $sid = $rule.IdentityReference.Value
    [Console]::Out.WriteLine(('RULE|{0}|{1}|{2}|{3}|{4}|{5}' -f `
        $sid,
        [int]$rule.AccessControlType,
        [int64]$rule.FileSystemRights,
        [int]$rule.InheritanceFlags,
        [int]$rule.PropagationFlags,
        [int]$rule.IsInherited))
}
";

const POWERSHELL_MAKE_PERMISSIVE_FILE_SCRIPT: &str = r"
$ErrorActionPreference = 'Stop'
$path = [System.Environment]::GetEnvironmentVariable('NAN_HARNESS_TEST_ACL_PATH')
if ([string]::IsNullOrWhiteSpace($path)) {
    throw 'NAN_HARNESS_TEST_ACL_PATH is required'
}

$item = [System.IO.FileInfo]::new($path)
$acl = $item.GetAccessControl()
$acl.SetAccessRuleProtection($false, $true)
$everyone = [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0')
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
    $everyone,
    [System.Security.AccessControl.FileSystemRights]::FullControl,
    [System.Security.AccessControl.AccessControlType]::Allow)
$acl.SetAccessRule($rule)
$item.SetAccessControl($acl)
";

const POWERSHELL_MAKE_PERMISSIVE_DIRECTORY_SCRIPT: &str = r"
$ErrorActionPreference = 'Stop'
$path = [System.Environment]::GetEnvironmentVariable('NAN_HARNESS_TEST_ACL_PATH')
if ([string]::IsNullOrWhiteSpace($path)) {
    throw 'NAN_HARNESS_TEST_ACL_PATH is required'
}

$item = [System.IO.DirectoryInfo]::new($path)
$acl = $item.GetAccessControl()
$acl.SetAccessRuleProtection($false, $true)
$everyone = [System.Security.Principal.SecurityIdentifier]::new('S-1-1-0')
$inheritance = [System.Security.AccessControl.InheritanceFlags](
    [int][System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
    [int][System.Security.AccessControl.InheritanceFlags]::ObjectInherit)
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
    $everyone,
    [System.Security.AccessControl.FileSystemRights]::FullControl,
    $inheritance,
    [System.Security.AccessControl.PropagationFlags]::None,
    [System.Security.AccessControl.AccessControlType]::Allow)
$acl.SetAccessRule($rule)
$item.SetAccessControl($acl)
";

#[derive(Debug)]
struct AclReport {
    current_user_sid: String,
    protected: bool,
    control_flags: i32,
    rules: Vec<AclRule>,
}

#[derive(Debug)]
struct AclRule {
    sid: String,
    access_control_type: i32,
    file_system_rights: i64,
    inheritance_flags: i32,
    propagation_flags: i32,
    is_inherited: bool,
}

fn run_powershell(script: &str, path: &Path) -> io::Result<()> {
    let output = Command::new("powershell.exe")
        .env(ACL_PATH_ENVIRONMENT_VARIABLE, path.as_os_str())
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "PowerShell ACL operation failed (status: {}): stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim(),
    )))
}

/// Make a file's ACL deliberately permissive for a Windows test fixture.
///
/// The helper is only available on Windows because it invokes the Windows
/// access-control APIs through `powershell.exe`.
///
/// # Errors
///
/// Returns an error when PowerShell cannot update or re-read the ACL, or when
/// the resulting ACL is still protected or lacks the expected `Everyone`
/// full-control rule.
pub fn make_permissive_file(path: &Path) -> io::Result<()> {
    run_powershell(POWERSHELL_MAKE_PERMISSIVE_FILE_SCRIPT, path)?;
    let report = read_acl(path)?;
    assert_permissive_rule(&report, NO_INHERITANCE)
}

/// Make a directory's ACL deliberately permissive for a Windows test fixture.
///
/// The helper is only available on Windows because it invokes the Windows
/// access-control APIs through `powershell.exe`.
///
/// # Errors
///
/// Returns an error when PowerShell cannot update or re-read the ACL, or when
/// the resulting ACL is still protected or lacks the expected inheritable
/// `Everyone` full-control rule.
pub fn make_permissive_directory(path: &Path) -> io::Result<()> {
    run_powershell(POWERSHELL_MAKE_PERMISSIVE_DIRECTORY_SCRIPT, path)?;
    let report = read_acl(path)?;
    assert_permissive_rule(&report, OBJECT_AND_CONTAINER_INHERIT)
}

fn parse_flag(value: &str, field: &str, line_number: usize) -> io::Result<bool> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(io::Error::other(format!(
            "invalid {field} value on ACL report line {line_number}: {value:?}"
        ))),
    }
}

fn parse_integer<T>(value: &str, field: &str, line_number: usize) -> io::Result<T>
where
    T: str::FromStr,
    T::Err: std::fmt::Display,
{
    value.parse().map_err(|error| {
        io::Error::other(format!(
            "invalid {field} value on ACL report line {line_number}: {value:?}: {error}"
        ))
    })
}

fn parse_acl_report(output: &[u8]) -> io::Result<AclReport> {
    let output = str::from_utf8(output)
        .map_err(|error| io::Error::other(format!("ACL report is not UTF-8: {error}")))?
        .trim_start_matches('\u{feff}');
    let mut current_user_sid = None;
    let mut protected = None;
    let mut control_flags = None;
    let mut rules = Vec::new();

    for (index, line) in output.lines().enumerate() {
        let line_number = index + 1;
        if line.is_empty() {
            continue;
        }
        let fields = line.split('|').collect::<Vec<_>>();
        match fields.as_slice() {
            ["CURRENT_USER_SID", sid] if !sid.is_empty() => {
                if current_user_sid.replace((*sid).to_owned()).is_some() {
                    return Err(io::Error::other(format!(
                        "duplicate CURRENT_USER_SID in ACL report line {line_number}"
                    )));
                }
            }
            ["PROTECTED", value] => {
                if protected
                    .replace(parse_flag(value, "PROTECTED", line_number)?)
                    .is_some()
                {
                    return Err(io::Error::other(format!(
                        "duplicate PROTECTED field in ACL report line {line_number}"
                    )));
                }
            }
            ["CONTROL_FLAGS", value] => {
                if control_flags
                    .replace(parse_integer(value, "CONTROL_FLAGS", line_number)?)
                    .is_some()
                {
                    return Err(io::Error::other(format!(
                        "duplicate CONTROL_FLAGS field in ACL report line {line_number}"
                    )));
                }
            }
            [
                "RULE",
                sid,
                access_type,
                rights,
                inheritance,
                propagation,
                inherited,
            ] if !sid.is_empty() => {
                rules.push(AclRule {
                    sid: (*sid).to_owned(),
                    access_control_type: parse_integer(
                        access_type,
                        "RULE access type",
                        line_number,
                    )?,
                    file_system_rights: parse_integer(
                        rights,
                        "RULE file-system rights",
                        line_number,
                    )?,
                    inheritance_flags: parse_integer(
                        inheritance,
                        "RULE inheritance flags",
                        line_number,
                    )?,
                    propagation_flags: parse_integer(
                        propagation,
                        "RULE propagation flags",
                        line_number,
                    )?,
                    is_inherited: parse_flag(inherited, "RULE inherited flag", line_number)?,
                });
            }
            _ => {
                return Err(io::Error::other(format!(
                    "invalid ACL report line {line_number}: {line:?}"
                )));
            }
        }
    }

    Ok(AclReport {
        current_user_sid: current_user_sid
            .ok_or_else(|| io::Error::other("ACL report did not contain CURRENT_USER_SID"))?,
        protected: protected
            .ok_or_else(|| io::Error::other("ACL report did not contain PROTECTED"))?,
        control_flags: control_flags
            .ok_or_else(|| io::Error::other("ACL report did not contain CONTROL_FLAGS"))?,
        rules,
    })
}

fn read_acl(path: &Path) -> io::Result<AclReport> {
    let output = Command::new("powershell.exe")
        .env(ACL_PATH_ENVIRONMENT_VARIABLE, path.as_os_str())
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            POWERSHELL_READ_ACL_SCRIPT,
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "PowerShell ACL inspection failed (status: {}): stdout: {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        )));
    }
    parse_acl_report(&output.stdout).map_err(|error| {
        io::Error::other(format!(
            "invalid PowerShell ACL report: {error}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        ))
    })
}

fn assert_permissive_rule(report: &AclReport, inheritance_flags: i32) -> io::Result<()> {
    if report.protected || report.control_flags & DACL_PROTECTED != 0 {
        return Err(io::Error::other(format!(
            "permissive ACL fixture unexpectedly has a protected DACL: {report:?}"
        )));
    }
    if !report.rules.iter().any(|rule| {
        rule.sid == EVERYONE_SID
            && rule.access_control_type == ALLOW
            && rule.file_system_rights == FULL_CONTROL
            && rule.inheritance_flags == inheritance_flags
            && rule.propagation_flags == NO_INHERITANCE
            && !rule.is_inherited
    }) {
        return Err(io::Error::other(format!(
            "permissive ACL fixture does not contain the expected Everyone full-control rule: {report:?}"
        )));
    }
    Ok(())
}

fn assert_allowed_rules(report: &AclReport, protected: bool) -> io::Result<()> {
    if report.protected != protected || (report.control_flags & DACL_PROTECTED != 0) != protected {
        return Err(io::Error::other(format!(
            "unexpected protected-DACL state (expected {protected}, got AreAccessRulesProtected={}, ControlFlags={}): {report:?}",
            report.protected, report.control_flags
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
