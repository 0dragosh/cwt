use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// A branch entry with metadata.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_remote: bool,
    pub is_current: bool,
}

/// List all local and remote branches.
pub fn list_branches(repo_root: &Path) -> Result<Vec<BranchInfo>> {
    let output = Command::new("git")
        .args(["branch", "-a", "--no-color"])
        .current_dir(repo_root)
        .output()
        .context("failed to run git branch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git branch -a failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.contains(" -> ") {
            continue;
        }

        let is_current = line.starts_with("* ");
        let name = line.trim_start_matches("* ").trim();

        if let Some(remote_name) = name.strip_prefix("remotes/") {
            branches.push(BranchInfo {
                name: remote_name.to_string(),
                is_remote: true,
                is_current: false,
            });
        } else {
            branches.push(BranchInfo {
                name: name.to_string(),
                is_remote: false,
                is_current,
            });
        }
    }

    Ok(branches)
}

/// Get the current branch name (or None if detached HEAD).
pub fn current_branch(repo_root: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(repo_root)
        .output()
        .context("failed to run git symbolic-ref")?;

    if output.status.success() {
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    } else {
        Ok(None) // detached HEAD
    }
}

/// Get the short hash of a ref.
pub fn short_hash(repo_root: &Path, refname: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", refname])
        .current_dir(repo_root)
        .output()
        .context("failed to run git rev-parse")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git rev-parse --short {} failed: {}", refname, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
