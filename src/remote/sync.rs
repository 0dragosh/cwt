use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::host::RemoteHost;

/// Sync direction for handoff across machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSyncDirection {
    /// Generate patch locally, apply on remote
    LocalToRemote,
    /// Generate patch on remote, apply locally
    RemoteToLocal,
}

/// Push the local branch to the remote repository via git push.
/// This is the primary sync mechanism for code changes.
pub fn git_push_to_remote(local_repo_root: &Path, branch: &str, remote_name: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["push", remote_name, branch])
        .current_dir(local_repo_root)
        .output()
        .context("failed to run git push")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git push failed: {}", stderr.trim());
    }

    Ok(())
}

/// Pull changes from a remote branch on the remote host.
pub fn git_pull_on_remote(
    host: &RemoteHost,
    repo_name: &str,
    worktree_name: &str,
    branch: &str,
) -> Result<()> {
    let repo_path = format!("{}/{}", host.worktree_dir, repo_name);
    let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);

    let cmd = format!(
        "cd {} && git pull origin {} --ff-only",
        ssh_shell_quote(&wt_path),
        ssh_shell_quote(branch)
    );
    host.ssh_exec(&cmd).with_context(|| {
        format!(
            "failed to pull on remote worktree '{}' on '{}'",
            worktree_name, host.name
        )
    })?;

    Ok(())
}

/// Generate a patch locally and apply it on the remote host.
pub fn handoff_local_to_remote(
    host: &RemoteHost,
    local_path: &Path,
    repo_name: &str,
    worktree_name: &str,
) -> Result<()> {
    // Generate patch from local changes
    let output = Command::new("git")
        .args(["diff"])
        .current_dir(local_path)
        .output()
        .context("failed to generate local diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git diff failed: {}", stderr.trim());
    }

    let patch = String::from_utf8_lossy(&output.stdout).to_string();
    if patch.trim().is_empty() {
        anyhow::bail!("no changes to transfer");
    }

    // Apply patch on remote
    apply_patch_on_remote(host, repo_name, worktree_name, &patch)
}

/// Generate a patch on the remote host and apply it locally.
pub fn handoff_remote_to_local(
    host: &RemoteHost,
    local_path: &Path,
    repo_name: &str,
    worktree_name: &str,
) -> Result<()> {
    // Get patch from remote
    let patch = host.diff_full(repo_name, worktree_name)?;

    if patch.trim().is_empty() {
        anyhow::bail!("no changes on remote worktree to transfer");
    }

    // Apply patch locally
    crate::git::commands::apply_patch(local_path, &patch)
        .context("failed to apply remote patch locally")?;

    Ok(())
}

/// Apply a patch on a remote worktree via SSH.
fn apply_patch_on_remote(
    host: &RemoteHost,
    repo_name: &str,
    worktree_name: &str,
    patch: &str,
) -> Result<()> {
    let repo_path = format!("{}/{}", host.worktree_dir, repo_name);
    let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);

    // Use SSH with stdin pipe to send the patch
    let mut ssh_cmd = Command::new("ssh");
    for arg in host.ssh_base_args() {
        ssh_cmd.arg(arg);
    }
    ssh_cmd.arg(host.ssh_dest());
    ssh_cmd.arg(format!("cd {} && git apply -", ssh_shell_quote(&wt_path)));
    ssh_cmd.stdin(std::process::Stdio::piped());
    ssh_cmd.stdout(std::process::Stdio::piped());
    ssh_cmd.stderr(std::process::Stdio::piped());

    let mut child = ssh_cmd
        .spawn()
        .context("failed to spawn SSH for patch apply")?;

    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        stdin
            .write_all(patch.as_bytes())
            .context("failed to write patch to SSH stdin")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait for SSH patch apply")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to apply patch on remote: {}", stderr.trim());
    }

    Ok(())
}

/// Sync state: fetch the list of worktrees on the remote host for a given repo.
pub fn list_remote_worktrees(host: &RemoteHost, repo_name: &str) -> Result<Vec<String>> {
    let repo_path = format!("{}/{}", host.worktree_dir, repo_name);
    let wt_dir = format!("{}/worktrees", repo_path);

    // Check if the worktrees directory exists
    let (stdout, _, success) = host.ssh_exec_fallible(&format!(
        "test -d {} && ls -1 {}",
        ssh_shell_quote(&wt_dir),
        ssh_shell_quote(&wt_dir)
    ))?;

    if !success {
        return Ok(Vec::new());
    }

    Ok(stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect())
}

/// Get the remote URL for the local repository (used for cloning on remote).
pub fn get_repo_remote_url(local_repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(local_repo_root)
        .output()
        .context("failed to get remote URL")?;

    if !output.status.success() {
        anyhow::bail!("no 'origin' remote configured");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Shell-quote a string for safe embedding in SSH commands.
fn ssh_shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Get the repository name from the local repo root path.
pub fn repo_name_from_path(local_repo_root: &Path) -> String {
    local_repo_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repo".to_string())
}
