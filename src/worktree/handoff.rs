use anyhow::{Context, Result};
use std::path::Path;

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
}

/// Generate a preview of the handoff operation.
pub fn preview(
    direction: HandoffDirection,
    worktree_path: &Path,
    local_path: &Path,
) -> Result<HandoffPreview> {
    let (stat, diff_text) = match direction {
        HandoffDirection::WorktreeToLocal => {
            let stat = git::diff::diff_stat(worktree_path)?;
            let diff = git::diff::diff_full(worktree_path)?;
            (stat, diff)
        }
        HandoffDirection::LocalToWorktree => {
            let stat = git::diff::diff_stat(local_path)?;
            let diff = git::diff::diff_full(local_path)?;
            (stat, diff)
        }
    };

    Ok(HandoffPreview {
        direction,
        diff_stat: stat,
        diff_text,
    })
}

/// Execute the handoff: generate a patch from the source and apply it to the target.
pub fn execute(
    direction: HandoffDirection,
    worktree_path: &Path,
    local_path: &Path,
) -> Result<()> {
    let (source, target) = match direction {
        HandoffDirection::WorktreeToLocal => (worktree_path, local_path),
        HandoffDirection::LocalToWorktree => (local_path, worktree_path),
    };

    let patch = git::diff::diff_full(source)
        .context("failed to generate diff from source")?;

    if patch.trim().is_empty() {
        anyhow::bail!("no changes to transfer");
    }

    git::commands::apply_patch(target, &patch)
        .context("failed to apply patch to target")?;

    Ok(())
}
