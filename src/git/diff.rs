use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Diff stat summary for a worktree.
#[derive(Debug, Clone, Default)]
pub struct DiffStat {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub raw: String,
}

/// Get `git diff --stat` for a worktree (uncommitted changes).
pub fn diff_stat(worktree_path: &Path) -> Result<DiffStat> {
    let output = Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .context("failed to run git diff --stat")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Only retry without HEAD if the error indicates missing HEAD (empty repo / unborn branch)
        if stderr.contains("unknown revision") || stderr.contains("bad revision") {
            let output = Command::new("git")
                .args(["diff", "--stat"])
                .current_dir(worktree_path)
                .output()
                .context("failed to run git diff --stat")?;

            let raw = String::from_utf8_lossy(&output.stdout).to_string();
            return Ok(parse_stat_summary(&raw));
        }
        // For other errors (permissions, corruption), propagate
        anyhow::bail!("git diff --stat HEAD failed: {}", stderr.trim());
    }

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(parse_stat_summary(&raw))
}

fn parse_stat_summary(raw: &str) -> DiffStat {
    let mut stat = DiffStat {
        raw: raw.to_string(),
        ..Default::default()
    };

    // The last line looks like: " 3 files changed, 10 insertions(+), 2 deletions(-)"
    if let Some(summary_line) = raw.lines().last() {
        for part in summary_line.split(',') {
            let part = part.trim();
            if part.contains("file") {
                stat.files_changed = part
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            } else if part.contains("insertion") {
                stat.insertions = part
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            } else if part.contains("deletion") {
                stat.deletions = part
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            }
        }
    }

    stat
}

fn run_git_checked(worktree_path: &Path, args: &[&str], context: &str) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(worktree_path)
        .output()
        .with_context(|| format!("failed to run {}", context))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} failed: {}", context, stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get the full diff for a worktree (uncommitted changes vs HEAD).
pub fn diff_full(worktree_path: &Path) -> Result<String> {
    run_git_checked(worktree_path, &["diff", "HEAD"], "git diff HEAD")
}

/// Get the diff between a base commit and the worktree's HEAD (committed changes).
pub fn diff_commits(worktree_path: &Path, base_commit: &str) -> Result<String> {
    run_git_checked(
        worktree_path,
        &["diff", base_commit, "HEAD"],
        "git diff <base> HEAD",
    )
}

/// Get `git log --oneline` between base and HEAD.
pub fn log_oneline(worktree_path: &Path, base_commit: &str) -> Result<String> {
    run_git_checked(
        worktree_path,
        &["log", "--oneline", &format!("{base_commit}..HEAD")],
        "git log --oneline",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_stat_summary() {
        let raw = " src/main.rs | 5 ++---\n src/lib.rs  | 2 ++\n 2 files changed, 4 insertions(+), 3 deletions(-)\n";
        let stat = parse_stat_summary(raw);
        assert_eq!(stat.files_changed, 2);
        assert_eq!(stat.insertions, 4);
        assert_eq!(stat.deletions, 3);
    }

    #[test]
    fn test_parse_stat_empty() {
        let stat = parse_stat_summary("");
        assert_eq!(stat.files_changed, 0);
    }

    #[test]
    fn diff_full_returns_error_outside_git_repo() {
        let tmp = TempDir::new().expect("create temp dir");
        let err = diff_full(tmp.path()).expect_err("expected git diff outside repo to fail");
        assert!(err.to_string().contains("git diff HEAD failed"));
    }

    #[test]
    fn diff_commits_returns_error_for_invalid_base_commit() {
        let tmp = TempDir::new().expect("create temp dir");

        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .expect("git init should run");

        std::fs::write(tmp.path().join("README.md"), "hello\n").expect("write README");
        Command::new("git")
            .args(["add", "."])
            .current_dir(tmp.path())
            .output()
            .expect("git add should run");
        Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "init",
            ])
            .current_dir(tmp.path())
            .output()
            .expect("git commit should run");

        let err = diff_commits(tmp.path(), "deadbeef")
            .expect_err("expected diff_commits with invalid commit to fail");
        assert!(err.to_string().contains("git diff <base> HEAD failed"));
    }
}
