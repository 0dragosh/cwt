use anyhow::{Context, Result};
use std::path::Path;

use crate::ship::pr::{self, CiStatus, PrStatus};
use crate::worktree::model::Worktree;

/// Result of the ship flow.
#[derive(Debug)]
pub struct ShipResult {
    pub pr_number: u64,
    pub pr_url: String,
    pub push_message: String,
}

/// Execute the "ship it" macro:
/// 1. Commit staged changes + push the worktree branch
/// 2. Create a PR
/// 3. Return the PR info so the caller can mark the worktree as "shipping"
pub fn ship(worktree: &Worktree, worktree_path: &Path) -> Result<ShipResult> {
    // Check gh is available
    if !pr::gh_available() {
        anyhow::bail!("gh CLI not found. Install it: https://cli.github.com/");
    }

    // Step 1: Commit + push
    let push_message = pr::commit_and_push(worktree_path, &worktree.branch)
        .context("failed to commit and push")?;

    // Step 2: Generate PR body from transcript
    let body = pr::generate_pr_body(worktree_path, worktree);

    // Step 3: Create PR
    let title = pr::generate_pr_title(worktree);
    let result = pr::create_pr(
        worktree_path,
        &worktree.branch,
        &worktree.base_branch,
        &title,
        &body,
    )
    .context("failed to create PR")?;

    Ok(ShipResult {
        pr_number: result.pr_number,
        pr_url: result.pr_url,
        push_message,
    })
}

/// Poll PR + CI status for a worktree, returning updated fields.
/// This is intended to be called periodically from the refresh loop.
pub fn poll_status(repo_path: &Path, worktree: &Worktree) -> (PrStatus, CiStatus, Option<String>) {
    let pr_number = worktree.pr_number.unwrap_or(0);
    if pr_number == 0 {
        return (PrStatus::None, CiStatus::None, None);
    }

    let (pr_status, pr_url) = pr::fetch_pr_status_by_number(repo_path, pr_number);
    let ci_status = crate::ship::ci::fetch_ci_status(repo_path, &worktree.branch);

    (pr_status, ci_status, pr_url)
}
