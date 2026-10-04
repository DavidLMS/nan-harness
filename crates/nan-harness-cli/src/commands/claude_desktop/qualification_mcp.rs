//! A hosted-only managed MCP fixture; configuration is not tool-use evidence.
use super::{ClaudeDesktopError, DesktopPaths, qualification_config};
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};

const SOURCE_HASH: &str = "ecb56f97d549f3040908f1bb8f0bb32235f9b48d9572ea348098135fe7999fc0";

fn regular_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.canonicalize().is_ok_and(|canonical| canonical == path)
        && std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
        && path
            .to_str()
            .is_some_and(|text| !text.chars().any(char::is_control))
}

fn fixture_file(workspace: &Path) -> Option<PathBuf> {
    if !qualification_config::private_directory(workspace)
        || workspace.canonicalize().ok()?.as_path() != workspace
    {
        return None;
    }
    let path = workspace.join("read-target.txt");
    if !regular_absolute(&path) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
            || metadata.uid() != std::fs::metadata(workspace).ok()?.uid()
        {
            return None;
        }
        // open_private_read independently checks ownership and private access.
        nan_harness_private_fs::open_private_read(&path).ok()?;
    }
    (metadata.len() <= 4096).then_some(path)
}

fn entry(workspace: &Path, interpreter: &Path, script: &Path, digest: &str) -> Option<Value> {
    let target = fixture_file(workspace)?;
    if !regular_absolute(interpreter) || !regular_absolute(script) || digest != SOURCE_HASH {
        return None;
    }
    let metadata = std::fs::metadata(script).ok()?;
    if metadata.len() > 32768 {
        return None;
    }
    let bytes = std::fs::read(script).ok()?;
    let expected = [
        0xec, 0xb5, 0x6f, 0x97, 0xd5, 0x49, 0xf3, 0x04, 0x09, 0x08, 0xf1, 0xbb, 0x8f, 0x0b, 0xb3,
        0x22, 0x35, 0xf9, 0xb4, 0x8d, 0x95, 0x72, 0xea, 0x34, 0x80, 0x98, 0x13, 0x5f, 0xe7, 0x99,
        0x9f, 0xc0,
    ];
    if Sha256::digest(bytes).as_slice() != expected {
        return None;
    }
    Some(json!({
        "name": "nanh-read-fixture", "transport": "stdio", "command": interpreter,
        "args": [script, "--workspace", workspace, "--file", target],
        "env": {}, "toolPolicy": {"read_file": "allow"}
    }))
}

pub(super) fn configure(
    paths: &DesktopPaths,
    profile: &mut Map<String, Value>,
) -> Result<(), ClaudeDesktopError> {
    let mac = std::env::var("NANH_CLAUDE_MCP_FIXTURE");
    let linux = std::env::var("NANH_CLAUDE_LINUX_MCP_FIXTURE");
    match (mac, linux) {
        (Err(std::env::VarError::NotPresent), Err(std::env::VarError::NotPresent)) => return Ok(()),
        (Ok(value), Err(std::env::VarError::NotPresent))
            if value == "read-only"
                && cfg!(target_os = "macos")
                && std::env::var("RUNNER_OS").as_deref() == Ok("macOS")
                && qualification_config::observation_directory(paths).is_some() => {}
        (Err(std::env::VarError::NotPresent), Ok(value))
            if value == "read-only" && super::qualification_linux::requested(paths)? => {}
        _ => return Err(ClaudeDesktopError::InvalidStatePath),
    }
    let resolve = |key| std::env::var_os(key).map(PathBuf::from);
    let configuration = (|| {
        entry(
            &std::env::current_dir().ok()?,
            &resolve("NANH_CLAUDE_MCP_PYTHON")?,
            &resolve("NANH_CLAUDE_MCP_SCRIPT")?,
            &std::env::var("NANH_CLAUDE_MCP_SOURCE_SHA256").ok()?,
        )
    })()
    .ok_or(ClaudeDesktopError::InvalidStatePath)?;
    let configuration = if std::env::var_os("NANH_CLAUDE_LINUX_MCP_FIXTURE").is_some() {
        linux_entry(configuration)?
    } else {
        configuration
    };
    // The enclosing desktop receipt snapshots and restores this whole document.
    install(profile, configuration)
}

fn linux_entry(mut configuration: Value) -> Result<Value, ClaudeDesktopError> {
    let args = configuration["args"]
        .as_array()
        .filter(|args| args.len() == 5)
        .ok_or(ClaudeDesktopError::InvalidStatePath)?;
    // Fixed Python code, never a shell or interpolated workspace/script text.
    // The fixture independently validates its original absolute root before reads.
    let script = args[0].clone();
    let workspace = args[2].clone();
    let target = args[4].clone();
    configuration["args"] = json!([
        "-I",
        "-c",
        "import os,runpy,sys; script,root,target=sys.argv[1:]; os.chdir(root); sys.argv=[script,'--workspace',root,'--file',target]; runpy.run_path(script,run_name='__main__')",
        script,
        workspace,
        target
    ]);
    Ok(configuration)
}

fn install(
    profile: &mut Map<String, Value>,
    configuration: Value,
) -> Result<(), ClaudeDesktopError> {
    let mut servers = match profile.get("managedMcpServers") {
        None => Vec::new(),
        Some(Value::Array(entries))
            if entries.len() < 100
                && entries.iter().all(|entry| {
                    entry.get("name").and_then(Value::as_str) != Some("nanh-read-fixture")
                }) =>
        {
            entries.clone()
        }
        _ => return Err(ClaudeDesktopError::InvalidStatePath),
    };
    servers.push(configuration);
    profile.insert("managedMcpServers".to_owned(), Value::Array(servers));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    #[test]
    fn linux_command_binds_private_cwd_without_shell_interpolation() {
        let configuration = json!({"args":["/trusted/script.py","--workspace","/owned/space $x","--file","/owned/space $x/read-target.txt"],"env":{},"toolPolicy":{"read_file":"allow"}});
        let result = linux_entry(configuration).expect("fixed Linux command");
        assert_eq!(result["args"][0], "-I");
        assert_eq!(result["args"][3], "/trusted/script.py");
        assert_eq!(result["args"][4], "/owned/space $x");
        assert!(!result["args"][2].as_str().unwrap().contains("$x"));
        assert_eq!(result["toolPolicy"]["read_file"], "allow");
        assert!(linux_entry(json!({"args":[]})).is_err());
    }

    #[test]
    fn managed_fixture_preserves_unrelated_configuration_and_rejects_collision() {
        let mut profile = Map::new();
        profile.insert("retained".into(), json!("private sentinel"));
        profile.insert("managedMcpServers".into(), json!([{"name":"existing"}]));
        install(&mut profile, json!({"name":"nanh-read-fixture"})).expect("new fixture");
        assert_eq!(profile["retained"], "private sentinel");
        assert_eq!(profile["managedMcpServers"][0]["name"], "existing");
        let before = profile.clone();
        assert!(install(&mut profile, json!({"name":"nanh-read-fixture"})).is_err());
        assert_eq!(profile, before);
    }

    #[test]
    fn fixture_rejects_external_links_and_nonprivate_files() {
        let directory = tempfile::tempdir().expect("fixture directory");
        let root = directory.path().canonicalize().expect("canonical fixture");
        nan_harness_private_fs::restrict_path(
            &root,
            nan_harness_private_fs::PrivatePathKind::Directory,
        )
        .expect("private root");
        let target = root.join("read-target.txt");
        let mut file = nan_harness_private_fs::open_private_new(&target).expect("private fixture");
        file.write_all(b"owned fixture").expect("fixture data");
        assert_eq!(fixture_file(&root), Some(target.clone()));
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/desktop-feasibility/claude-read-fixture.py");
        let script = root.join("fixture.py");
        std::fs::copy(source, &script).expect("public fixture source");
        let interpreter = std::env::current_exe().expect("synthetic executable");
        let configuration =
            entry(&root, &interpreter, &script, SOURCE_HASH).expect("pinned configuration");
        assert_eq!(configuration["transport"], "stdio");
        assert_eq!(configuration["toolPolicy"]["read_file"], "allow");
        assert_eq!(
            configuration["args"][4],
            target.to_str().expect("fixture path")
        );
        assert!(entry(&root, &interpreter, &script, "wrong digest").is_none());
        std::fs::write(&script, b"untrusted source sentinel").expect("replace synthetic script");
        assert!(entry(&root, &interpreter, &script, SOURCE_HASH).is_none());

        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt as _, symlink};
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))
                .expect("shared fixture");
            assert!(fixture_file(&root).is_none());
            std::fs::remove_file(&target).expect("remove fixture");
            symlink(root.join("outside"), &target).expect("synthetic link");
            assert!(fixture_file(&root).is_none());
        }
        assert!(
            entry(
                &root,
                Path::new("relative"),
                Path::new("relative"),
                SOURCE_HASH
            )
            .is_none()
        );
    }
}
