use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::git;

/// Direction of a handoff operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffDirection {
    /// Apply worktree changes to local working directory
    WorktreeToLocal,
    /// Send local changes to worktree
    LocalToWorktree,
}

/// Preview of what a handoff will do.
#[derive(Debug, Clone)]
pub struct HandoffPreview {
    pub direction: HandoffDirection,
    pub diff_stat: git::diff::DiffStat,
    pub diff_text: String,
    pub has_commits: bool,
    pub commit_count: usize,
    pub gitignore_warnings: Vec<String>,
}

/// Check for untracked files in the source that won't be transferred.
fn check_gitignore_warnings(source_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(source_path)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let files = String::from_utf8_lossy(&out.stdout);
            files
                .lines()
                .filter(|l| !l.is_empty())
                .map(|f| f.to_string())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Count commits in the worktree beyond the base commit.
fn count_commits_since_base(worktree_path: &Path, base_commit: &str) -> usize {
    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{base_commit}..HEAD")])
        .current_dir(worktree_path)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        _ => 0,
    }
}

/// Generate a preview of the handoff operation.
pub fn preview(
    direction: HandoffDirection,
    worktree_path: &Path,
    local_path: &Path,
    base_commit: Option<&str>,
) -> Result<HandoffPreview> {
    let (source_path, stat, diff_text) = match direction {
        HandoffDirection::WorktreeToLocal => {
            let stat = git::diff::diff_stat(worktree_path)?;
            let diff = git::diff::diff_full(worktree_path)?;
            (worktree_path, stat, diff)
        }
        HandoffDirection::LocalToWorktree => {
            let stat = git::diff::diff_stat(local_path)?;
            let diff = git::diff::diff_full(local_path)?;
            (local_path, stat, diff)
        }
    };

    let gitignore_warnings = check_gitignore_warnings(source_path);

    let (has_commits, commit_count) = if direction == HandoffDirection::WorktreeToLocal {
        if let Some(base) = base_commit {
            let count = count_commits_since_base(worktree_path, base);
            (count > 0, count)
        } else {
            (false, 0)
        }
    } else {
        (false, 0)
    };

    Ok(HandoffPreview {
        direction,
        diff_stat: stat,
        diff_text,
        has_commits,
        commit_count,
        gitignore_warnings,
    })
}

/// Generate format-patch output for commits since base.
fn format_patch_commits(worktree_path: &Path, base_commit: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["format-patch", "--stdout", &format!("{base_commit}..HEAD")])
        .current_dir(worktree_path)
        .output()
        .context("failed to run git format-patch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git format-patch failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Apply mailbox-format patches using git am.
fn apply_mailbox(target_path: &Path, mbox_content: &str) -> Result<()> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["am", "--3way"])
        .current_dir(target_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to run git am")?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(mbox_content.as_bytes())
            .context("failed to write patches to git am stdin")?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = Command::new("git")
            .args(["am", "--abort"])
            .current_dir(target_path)
            .output();
        anyhow::bail!("git am failed: {}", stderr.trim());
    }

    Ok(())
}

/// Execute the handoff: transfer changes from source to target.
/// For WT->Local with commits: uses format-patch/am for committed changes,
/// then applies uncommitted changes as a patch.
pub fn execute(
    direction: HandoffDirection,
    worktree_path: &Path,
    local_path: &Path,
    base_commit: Option<&str>,
) -> Result<()> {
    match direction {
        HandoffDirection::WorktreeToLocal => {
            // Transfer committed changes via format-patch/am
            if let Some(base) = base_commit {
                let commit_count = count_commits_since_base(worktree_path, base);
                if commit_count > 0 {
                    let mbox = format_patch_commits(worktree_path, base)
                        .context("failed to generate format-patch")?;
                    if !mbox.trim().is_empty() {
                        apply_mailbox(local_path, &mbox)
                            .context("failed to apply commits to local")?;
                    }
                }
            }

            // Then apply uncommitted changes as a patch
            let patch = git::diff::diff_full(worktree_path)
                .context("failed to generate diff from worktree")?;
            if !patch.trim().is_empty() {
                git::commands::apply_patch(local_path, &patch)
                    .context("failed to apply uncommitted changes to local")?;
            }
        }
        HandoffDirection::LocalToWorktree => {
            let patch = git::diff::diff_full(local_path)
                .context("failed to generate diff from local")?;
            if patch.trim().is_empty() {
                anyhow::bail!("no changes to transfer");
            }
            git::commands::apply_patch(worktree_path, &patch)
                .context("failed to apply patch to worktree")?;
        }
    }

    Ok(())
}
