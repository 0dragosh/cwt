use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Parsed worktree entry from `git worktree list --porcelain`.
#[derive(Debug, Clone)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub is_bare: bool,
}

/// Run a git command in the given directory and return stdout.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a git command, returning Ok even on non-zero exit (returns (stdout, stderr, success)).
fn git_fallible(dir: &Path, args: &[&str]) -> Result<(String, String, bool)> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    Ok((
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    ))
}

/// List all git worktrees using `--porcelain` format.
pub fn worktree_list(repo_root: &Path) -> Result<Vec<GitWorktree>> {
    let output = git(repo_root, &["worktree", "list", "--porcelain"])?;
    parse_porcelain(&output)
}

fn parse_porcelain(output: &str) -> Result<Vec<GitWorktree>> {
    let mut worktrees = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut head: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut is_bare = false;

    for line in output.lines() {
        if line.starts_with("worktree ") {
            // Save previous entry if any
            if let (Some(p), Some(h)) = (path.take(), head.take()) {
                worktrees.push(GitWorktree {
                    path: p,
                    head: h,
                    branch: branch.take(),
                    is_bare,
                });
                is_bare = false;
            }
            path = Some(PathBuf::from(line.trim_start_matches("worktree ")));
        } else if line.starts_with("HEAD ") {
            head = Some(line.trim_start_matches("HEAD ").to_string());
        } else if line.starts_with("branch ") {
            let b = line.trim_start_matches("branch refs/heads/").to_string();
            branch = Some(b);
        } else if line == "bare" {
            is_bare = true;
        }
        // blank line separates entries — we handle this via the "worktree " prefix
    }

    // Push the last entry
    if let (Some(p), Some(h)) = (path, head) {
        worktrees.push(GitWorktree {
            path: p,
            head: h,
            branch,
            is_bare,
        });
    }

    Ok(worktrees)
}

/// Add a new worktree with a new branch.
pub fn worktree_add(
    repo_root: &Path,
    worktree_path: &Path,
    branch: &str,
    base: &str,
) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let path_str = worktree_path
        .to_str()
        .context("worktree path is not valid UTF-8")?;

    git(
        repo_root,
        &["worktree", "add", path_str, "-b", branch, base],
    )?;
    Ok(())
}

/// Remove a worktree.
pub fn worktree_remove(repo_root: &Path, worktree_path: &Path, force: bool) -> Result<()> {
    let path_str = worktree_path
        .to_str()
        .context("worktree path is not valid UTF-8")?;

    let mut args = vec!["worktree", "remove", path_str];
    if force {
        args.push("--force");
    }

    git(repo_root, &args)?;
    Ok(())
}

/// Prune stale worktree metadata (e.g. after manual directory removal).
pub fn worktree_prune(repo_root: &Path) -> Result<()> {
    git(repo_root, &["worktree", "prune"])?;
    Ok(())
}

/// Delete a branch.
pub fn branch_delete(repo_root: &Path, branch: &str, force: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    git(repo_root, &["branch", flag, branch])?;
    Ok(())
}

/// Get the repo root directory from any path within the repo.
pub fn repo_root(path: &Path) -> Result<PathBuf> {
    let output = git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(output.trim()))
}

/// Get the canonical "common" repo root directory for a worktree or main checkout.
///
/// Unlike `repo_root`, this resolves to the shared repository root even when called
/// from inside a linked worktree.
pub fn common_repo_root(path: &Path) -> Result<PathBuf> {
    let output = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    repo_root_from_common_dir(Path::new(output.trim()))
}

fn repo_root_from_common_dir(common_dir: &Path) -> Result<PathBuf> {
    let git_dir_name = std::ffi::OsStr::new(".git");
    if common_dir.file_name() == Some(git_dir_name) {
        return common_dir
            .parent()
            .map(Path::to_path_buf)
            .context("git common dir has no parent");
    }

    // Fallback for unusual layouts (e.g., bare repos): use the common dir as-is.
    Ok(common_dir.to_path_buf())
}

/// Get the HEAD commit hash.
pub fn head_commit(repo_root: &Path) -> Result<String> {
    let output = git(repo_root, &["rev-parse", "HEAD"])?;
    Ok(output.trim().to_string())
}

/// Check if working directory has uncommitted changes.
pub fn is_dirty(repo_root: &Path) -> Result<bool> {
    let output = git(repo_root, &["status", "--porcelain"])?;
    Ok(!output.trim().is_empty())
}

/// Check if a branch has unpushed commits relative to its upstream.
pub fn has_unpushed_commits(repo_root: &Path, branch: &str) -> Result<bool> {
    let (stdout, _stderr, success) = git_fallible(
        repo_root,
        &["log", &format!("@{{upstream}}..{branch}"), "--oneline"],
    )?;
    if !success {
        // No upstream configured — consider as "has unpushed" to be safe
        return Ok(true);
    }
    Ok(!stdout.trim().is_empty())
}

/// Stash changes in the given directory. Returns true if something was stashed.
pub fn stash(dir: &Path) -> Result<bool> {
    // Check if there's anything to stash first to avoid false positives
    // from concurrent stash operations by other processes
    let is_dirty = is_dirty(dir)?;
    if !is_dirty {
        return Ok(false);
    }
    git(dir, &["stash", "push", "-u", "-m", "cwt: carry changes"])?;
    // If the command succeeded and we had changes, we stashed something
    Ok(true)
}

/// Pop the top stash entry.
pub fn stash_pop(dir: &Path) -> Result<()> {
    git(dir, &["stash", "pop"])?;
    Ok(())
}

/// Apply a patch file using git apply.
pub fn apply_patch(dir: &Path, patch_content: &str) -> Result<()> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["apply", "--3way", "-"])
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to run git apply")?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(patch_content.as_bytes())
            .context("failed to write patch to git apply stdin")?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git apply failed: {}", stderr.trim());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_porcelain() {
        let input = "\
worktree /home/user/project
HEAD abc1234567890
branch refs/heads/main

worktree /home/user/project/.claude/worktrees/feat
HEAD def4567890123
branch refs/heads/wt/feat

";
        let wts = parse_porcelain(input).unwrap();
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[0].path, PathBuf::from("/home/user/project"));
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[1].branch.as_deref(), Some("wt/feat"));
        assert!(!wts[0].is_bare);
    }

    #[test]
    fn test_parse_porcelain_bare() {
        let input = "\
worktree /home/user/project.git
HEAD abc1234567890
bare

";
        let wts = parse_porcelain(input).unwrap();
        assert_eq!(wts.len(), 1);
        assert!(wts[0].is_bare);
    }

    #[test]
    fn test_repo_root_from_common_dir_standard() {
        let root = repo_root_from_common_dir(Path::new("/tmp/proj/.git")).unwrap();
        assert_eq!(root, PathBuf::from("/tmp/proj"));
    }

    #[test]
    fn test_repo_root_from_common_dir_bare_fallback() {
        let root = repo_root_from_common_dir(Path::new("/tmp/proj.git")).unwrap();
        assert_eq!(root, PathBuf::from("/tmp/proj.git"));
    }
}
