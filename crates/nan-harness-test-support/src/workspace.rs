use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use thiserror::Error;

#[derive(Debug)]
pub struct ConformanceWorkspace {
    root: TempDir,
    claude_config: TempDir,
}

impl ConformanceWorkspace {
    /// Creates a disposable Git workspace containing fixtures for Claude Code tools.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] when a directory, fixture, or Git repository cannot be created.
    pub fn create() -> Result<Self, WorkspaceError> {
        let root = tempfile::tempdir().map_err(WorkspaceError::CreateRoot)?;
        let claude_config = tempfile::tempdir().map_err(WorkspaceError::CreateConfig)?;
        write(root.path().join("read-target.txt"), "READ_TARGET_CONTENT\n")?;
        write(root.path().join("edit-target.txt"), "EDIT_TARGET_BEFORE\n")?;
        write(
            root.path().join("notebook-target.ipynb"),
            r#"{"cells":[{"cell_type":"markdown","metadata":{},"source":["NOTEBOOK_BEFORE"]}],"metadata":{},"nbformat":4,"nbformat_minor":5}"#,
        )?;
        write(
            root.path().join(".claude/skills/conformance/SKILL.md"),
            "---\nname: conformance\ndescription: Return the conformance marker.\n---\n\nReturn SKILL_CONFORMANCE_OK.\n",
        )?;
        write(
            root.path().join("CLAUDE.md"),
            "This is an isolated NaN Harness conformance workspace.\n",
        )?;
        let output = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root.path())
            .output()
            .map_err(WorkspaceError::StartGit)?;
        if !output.status.success() {
            return Err(WorkspaceError::GitFailed(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        run_git(root.path(), &["add", "."])?;
        run_git(
            root.path(),
            &[
                "-c",
                "user.name=NaN Harness Tests",
                "-c",
                "user.email=nan-harness@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "test: create conformance fixture",
            ],
        )?;
        Ok(Self {
            root,
            claude_config,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        self.root.path()
    }

    #[must_use]
    pub fn claude_config_path(&self) -> &Path {
        self.claude_config.path()
    }

    #[must_use]
    pub fn resolve(&self, relative_path: impl AsRef<Path>) -> PathBuf {
        self.path().join(relative_path)
    }

    /// Reads a UTF-8 fixture from the workspace.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceError`] when the file cannot be read.
    pub fn read(&self, relative_path: impl AsRef<Path>) -> Result<String, WorkspaceError> {
        let path = self.resolve(relative_path);
        fs::read_to_string(&path).map_err(|source| WorkspaceError::Read { path, source })
    }
}

fn run_git(current_directory: &Path, arguments: &[&str]) -> Result<(), WorkspaceError> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(current_directory)
        .output()
        .map_err(WorkspaceError::StartGit)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WorkspaceError::GitFailed(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

fn write(path: PathBuf, contents: &str) -> Result<(), WorkspaceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| WorkspaceError::CreateDirectory {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&path, contents).map_err(|source| WorkspaceError::Write { path, source })
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("could not create the temporary workspace: {0}")]
    CreateRoot(std::io::Error),
    #[error("could not create the temporary Claude configuration: {0}")]
    CreateConfig(std::io::Error),
    #[error("could not create fixture directory '{}': {source}", path.display())]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write fixture '{}': {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read fixture '{}': {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not start git for the temporary workspace: {0}")]
    StartGit(std::io::Error),
    #[error("git could not initialize the temporary workspace: {0}")]
    GitFailed(String),
}
