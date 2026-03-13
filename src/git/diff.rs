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

/// Get the full diff for a worktree (uncommitted changes vs HEAD).
pub fn diff_full(worktree_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .context("failed to run git diff")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get the diff between a base commit and the worktree's HEAD (committed changes).
pub fn diff_commits(worktree_path: &Path, base_commit: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", base_commit, "HEAD"])
        .current_dir(worktree_path)
        .output()
        .context("failed to run git diff")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get `git log --oneline` between base and HEAD.
pub fn log_oneline(worktree_path: &Path, base_commit: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("{base_commit}..HEAD")])
        .current_dir(worktree_path)
        .output()
        .context("failed to run git log")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
