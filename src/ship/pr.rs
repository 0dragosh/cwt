use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::session;
use crate::worktree::model::Worktree;

/// PR status as reported by GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrStatus {
    #[default]
    None,
    Draft,
    Open,
    Approved,
    Merged,
    Closed,
}

impl PrStatus {
    /// Icon for display in the worktree list.
    pub fn icon(&self) -> &'static str {
        match self {
            PrStatus::None => "",
            PrStatus::Draft => "PR:draft",
            PrStatus::Open => "PR:open",
            PrStatus::Approved => "PR:ok",
            PrStatus::Merged => "PR:merged",
            PrStatus::Closed => "PR:closed",
        }
    }

    /// Short label for the inspector.
    pub fn label(&self) -> &'static str {
        match self {
            PrStatus::None => "none",
            PrStatus::Draft => "draft",
            PrStatus::Open => "open / in review",
            PrStatus::Approved => "approved",
            PrStatus::Merged => "merged",
            PrStatus::Closed => "closed",
        }
    }
}

/// CI check status.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CiStatus {
    #[default]
    None,
    Pending,
    Passed,
    Failed,
}

impl CiStatus {
    /// Icon for display.
    pub fn icon(&self) -> &'static str {
        match self {
            CiStatus::None => "",
            CiStatus::Pending => "CI:...",
            CiStatus::Passed => "CI:ok",
            CiStatus::Failed => "CI:fail",
        }
    }
}

/// Result of creating a PR.
#[derive(Debug, Clone)]
pub struct PrCreateResult {
    pub pr_number: u64,
    pub pr_url: String,
}

/// Check if `gh` CLI is available.
pub fn gh_available() -> bool {
    which::which("gh").is_ok()
}

/// Check if `gh` is authenticated.
pub fn gh_authenticated() -> bool {
    Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Stage all changes, commit, and push the worktree branch.
/// Returns the commit message used.
pub fn commit_and_push(worktree_path: &Path, branch: &str) -> Result<String> {
    // Check for changes to commit
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .output()
        .context("failed to run git status")?;
    let status = String::from_utf8_lossy(&status_output.stdout);

    let has_changes = !status.trim().is_empty();

    if has_changes {
        // Stage all changes
        let add_output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(worktree_path)
            .output()
            .context("failed to run git add")?;
        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            anyhow::bail!("git add failed: {}", stderr.trim());
        }

        // Generate a commit message from the diff stat
        let stat = Command::new("git")
            .args(["diff", "--cached", "--stat"])
            .current_dir(worktree_path)
            .output()
            .context("failed to get diff stat")?;
        let stat_str = String::from_utf8_lossy(&stat.stdout);
        let summary_line = stat_str
            .lines()
            .last()
            .unwrap_or("changes")
            .trim()
            .to_string();

        let commit_msg = format!("cwt: {}", summary_line);

        // Commit
        let commit_output = Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(worktree_path)
            .output()
            .context("failed to run git commit")?;
        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            anyhow::bail!("git commit failed: {}", stderr.trim());
        }
    }

    // Push the branch (set upstream if needed)
    let push_output = Command::new("git")
        .args(["push", "-u", "origin", branch])
        .current_dir(worktree_path)
        .output()
        .context("failed to run git push")?;
    if !push_output.status.success() {
        let stderr = String::from_utf8_lossy(&push_output.stderr);
        anyhow::bail!("git push failed: {}", stderr.trim());
    }

    if has_changes {
        Ok("Committed and pushed".to_string())
    } else {
        Ok("Pushed (no new changes to commit)".to_string())
    }
}

/// Generate a PR body from the session transcript.
/// Reads the last few assistant messages and creates a summary.
pub fn generate_pr_body(worktree_path: &Path, worktree: &Worktree) -> String {
    let mut body = String::new();
    body.push_str("## Summary\n\n");

    // Try to get transcript summary
    let transcript_summary = session::tracker::find_project_dir(worktree_path)
        .ok()
        .flatten()
        .and_then(|dir| session::transcript::read_last_messages(&dir, 3).ok())
        .map(|messages| {
            let mut summary = String::new();
            for msg in &messages {
                // Take the first ~200 chars of each message
                let content = if msg.content.len() > 200 {
                    format!("{}...", &msg.content[..200])
                } else {
                    msg.content.clone()
                };
                summary.push_str(&format!("- {}\n", content.replace('\n', " ")));
            }
            summary
        });

    if let Some(summary) = transcript_summary {
        if !summary.is_empty() {
            body.push_str("From Claude session transcript:\n\n");
            body.push_str(&summary);
            body.push('\n');
        } else {
            body.push_str("_Changes from cwt worktree._\n\n");
        }
    } else {
        body.push_str("_Changes from cwt worktree._\n\n");
    }

    body.push_str("## Details\n\n");
    body.push_str(&format!("- **Worktree**: `{}`\n", worktree.name));
    body.push_str(&format!("- **Branch**: `{}`\n", worktree.branch));
    body.push_str(&format!("- **Base**: `{}`\n", worktree.base_branch));
    body.push_str(&format!(
        "- **Base commit**: `{}`\n",
        &worktree.base_commit[..8.min(worktree.base_commit.len())]
    ));

    body
}

/// Create a PR using `gh pr create`.
/// Returns the PR number and URL.
pub fn create_pr(
    worktree_path: &Path,
    branch: &str,
    base_branch: &str,
    title: &str,
    body: &str,
) -> Result<PrCreateResult> {
    if !gh_available() {
        anyhow::bail!("gh CLI not found. Install it: https://cli.github.com/");
    }

    let output = Command::new("gh")
        .args([
            "pr",
            "create",
            "--head",
            branch,
            "--base",
            base_branch,
            "--title",
            title,
            "--body",
            body,
        ])
        .current_dir(worktree_path)
        .output()
        .context("failed to run gh pr create")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr create failed: {}", stderr.trim());
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Extract PR number from URL (e.g., https://github.com/owner/repo/pull/42)
    let pr_number = url
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    Ok(PrCreateResult {
        pr_number,
        pr_url: url,
    })
}

/// Fetch the current PR status for a branch using `gh pr view`.
pub fn fetch_pr_status(repo_path: &Path, branch: &str) -> (PrStatus, Option<u64>, Option<String>) {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,state,url,isDraft,reviewDecision",
        ])
        .current_dir(repo_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return (PrStatus::None, None, None),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return (PrStatus::None, None, None),
    };

    let number = json.get("number").and_then(|v| v.as_u64());
    let url = json
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let state = json.get("state").and_then(|v| v.as_str()).unwrap_or("");
    let is_draft = json
        .get("isDraft")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let review_decision = json
        .get("reviewDecision")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let status = match state {
        "MERGED" => PrStatus::Merged,
        "CLOSED" => PrStatus::Closed,
        "OPEN" => {
            if is_draft {
                PrStatus::Draft
            } else if review_decision == "APPROVED" {
                PrStatus::Approved
            } else {
                PrStatus::Open
            }
        }
        _ => PrStatus::None,
    };

    (status, number, url)
}

/// Fetch PR status by PR number (for known PRs).
pub fn fetch_pr_status_by_number(repo_path: &Path, pr_number: u64) -> (PrStatus, Option<String>) {
    let output = Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "state,url,isDraft,reviewDecision",
        ])
        .current_dir(repo_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return (PrStatus::None, None),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return (PrStatus::None, None),
    };

    let url = json
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let state = json.get("state").and_then(|v| v.as_str()).unwrap_or("");
    let is_draft = json
        .get("isDraft")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let review_decision = json
        .get("reviewDecision")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let status = match state {
        "MERGED" => PrStatus::Merged,
        "CLOSED" => PrStatus::Closed,
        "OPEN" => {
            if is_draft {
                PrStatus::Draft
            } else if review_decision == "APPROVED" {
                PrStatus::Approved
            } else {
                PrStatus::Open
            }
        }
        _ => PrStatus::None,
    };

    (status, url)
}
