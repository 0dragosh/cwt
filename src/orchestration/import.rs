use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::orchestration::dispatch;
use crate::worktree::Manager;

/// A single issue from an external tracker.
#[derive(Debug, Clone)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub url: String,
}

/// Import result for a single issue.
#[derive(Debug)]
pub struct ImportResult {
    pub issue: Issue,
    pub worktree_name: String,
    pub pane_id: Option<String>,
    pub error: Option<String>,
}

/// Fetch open GitHub issues using the `gh` CLI.
pub fn fetch_github_issues(repo_root: &Path, limit: usize) -> Result<Vec<Issue>> {
    // Check that gh is available
    which::which("gh").context("gh CLI not found on PATH. Install it: https://cli.github.com/")?;

    let output = Command::new("gh")
        .args([
            "issue",
            "list",
            "--state",
            "open",
            "--limit",
            &limit.to_string(),
            "--json",
            "number,title,body,labels,url",
        ])
        .current_dir(repo_root)
        .output()
        .context("failed to run gh issue list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh issue list failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).context("failed to parse gh output as JSON")?;

    let issues = json
        .as_array()
        .context("expected JSON array from gh")?
        .iter()
        .filter_map(|v| {
            let number = v.get("number")?.as_u64()?;
            let title = v.get("title")?.as_str()?.to_string();
            let body = v
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string();
            let url = v
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            let labels = v
                .get("labels")
                .and_then(|l| l.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|lv| {
                            lv.get("name")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(Issue {
                number,
                title,
                body,
                labels,
                url,
            })
        })
        .collect();

    Ok(issues)
}

/// Fetch issues from Linear using the Linear CLI/API.
/// Uses `curl` with the Linear API since there's no official CLI.
/// Requires LINEAR_API_KEY environment variable.
pub fn fetch_linear_issues(limit: usize) -> Result<Vec<Issue>> {
    let api_key = std::env::var("LINEAR_API_KEY").context(
        "LINEAR_API_KEY environment variable not set. Get your API key from Linear Settings > API.",
    )?;

    let query = format!(
        r#"{{"query": "query {{ issues(first: {}, filter: {{ state: {{ type: {{ in: [\"started\", \"unstarted\", \"backlog\"] }} }} }}) {{ nodes {{ number title description url labels {{ nodes {{ name }} }} }} }} }}"}}"#,
        limit
    );

    // Pass the API key via stdin using -K - to avoid exposing it in process args (ps aux)
    use std::io::Write;
    let mut child = Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-K", "-", // Read config from stdin
            "-d",
            &query,
            "https://api.linear.app/graphql",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn curl for Linear API")?;

    if let Some(ref mut stdin) = child.stdin {
        let _ = writeln!(stdin, "header = \"Authorization: {}\"", api_key);
    }
    // Drop stdin to signal EOF
    drop(child.stdin.take());

    let output = child.wait_with_output()
        .context("failed to call Linear API")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Linear API call failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).context("failed to parse Linear API response")?;

    let nodes = json
        .get("data")
        .and_then(|d| d.get("issues"))
        .and_then(|i| i.get("nodes"))
        .and_then(|n| n.as_array())
        .context("unexpected Linear API response structure")?;

    let issues = nodes
        .iter()
        .filter_map(|v| {
            let number = v.get("number")?.as_u64()?;
            let title = v.get("title")?.as_str()?.to_string();
            let body = v
                .get("description")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string();
            let url = v
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            let labels = v
                .get("labels")
                .and_then(|l| l.get("nodes"))
                .and_then(|n| n.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|lv| {
                            lv.get("name")
                                .and_then(|n| n.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(Issue {
                number,
                title,
                body,
                labels,
                url,
            })
        })
        .collect();

    Ok(issues)
}

/// Import issues: create a worktree per issue and launch Claude with a prompt
/// that includes the issue context and a "Fixes #N" instruction.
pub fn import_issues(
    manager: &Manager,
    issues: &[Issue],
    base_branch: &str,
    source: &str,
) -> Vec<ImportResult> {
    issues
        .iter()
        .map(|issue| {
            let prompt = build_issue_prompt(issue, source);
            let result =
                dispatch::dispatch_tasks(manager, std::slice::from_ref(&prompt), base_branch);
            let dr = match result.into_iter().next() {
                Some(dr) => dr,
                None => {
                    return ImportResult {
                        issue: issue.clone(),
                        worktree_name: String::new(),
                        pane_id: None,
                        error: Some("dispatch returned no results".to_string()),
                    };
                }
            };

            ImportResult {
                issue: issue.clone(),
                worktree_name: dr.worktree_name,
                pane_id: dr.pane_id,
                error: dr.error,
            }
        })
        .collect()
}

/// Build a prompt string for an issue that includes context and Fixes #N.
fn build_issue_prompt(issue: &Issue, source: &str) -> String {
    let mut prompt = format!(
        "Implement the following {} issue.\n\n\
         Issue #{}: {}\n",
        source, issue.number, issue.title
    );

    if !issue.body.is_empty() {
        // Truncate very long bodies (char-safe to avoid panic on multi-byte chars)
        let body = if issue.body.chars().count() > 2000 {
            let truncated: String = issue.body.chars().take(2000).collect();
            format!("{}...", truncated)
        } else {
            issue.body.clone()
        };
        prompt.push_str(&format!("\nDescription:\n{}\n", body));
    }

    if !issue.labels.is_empty() {
        prompt.push_str(&format!("\nLabels: {}\n", issue.labels.join(", ")));
    }

    prompt.push_str(&format!(
        "\nWhen committing, include 'Fixes #{}' in the commit message.",
        issue.number
    ));

    prompt
}
