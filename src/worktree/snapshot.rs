use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

use crate::git::diff;
use crate::state::SnapshotEntry;
use crate::worktree::model::Worktree;

/// Directory where snapshots are stored.
fn snapshot_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".cwt/snapshots"))
}

/// Save a snapshot of the worktree's changes before deletion.
/// Returns the snapshot entry to be stored in state.
pub fn save_snapshot(worktree: &Worktree, repo_root: &Path) -> Result<SnapshotEntry> {
    let dir = snapshot_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create snapshot dir {}", dir.display()))?;

    let wt_abs_path = if worktree.path.is_relative() {
        repo_root.join(&worktree.path)
    } else {
        worktree.path.clone()
    };

    // Capture uncommitted changes
    let uncommitted_diff = diff::diff_full(&wt_abs_path)?;

    // Capture committed changes since base
    let committed_diff = diff::diff_commits(&wt_abs_path, &worktree.base_commit)?;

    // Capture commit log
    let log = diff::log_oneline(&wt_abs_path, &worktree.base_commit)?;

    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    // Sanitize worktree name to prevent path traversal (e.g., "../../etc/cron.d/evil")
    let safe_name: String = worktree
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let patch_filename = format!("{}-{}.patch", safe_name, timestamp);
    let patch_path = dir.join(&patch_filename);

    // Verify the path stays within the snapshot directory
    if !patch_path.starts_with(&dir) {
        anyhow::bail!(
            "snapshot path '{}' escapes snapshot directory",
            patch_path.display()
        );
    }

    // Write combined patch: committed changes + uncommitted changes
    let mut content = String::new();
    content.push_str(&format!("# cwt snapshot: {}\n", worktree.name));
    content.push_str(&format!("# base branch: {}\n", worktree.base_branch));
    content.push_str(&format!("# base commit: {}\n", worktree.base_commit));
    content.push_str(&format!("# created: {}\n", worktree.created_at));
    content.push_str(&format!("# snapshot: {}\n", Utc::now()));

    if !log.trim().is_empty() {
        content.push_str("\n# Commits since base:\n");
        for line in log.lines() {
            content.push_str(&format!("# {}\n", line));
        }
    }

    if !committed_diff.is_empty() {
        content.push_str("\n### Committed changes ###\n");
        content.push_str(&committed_diff);
    }

    if !uncommitted_diff.is_empty() {
        content.push_str("\n### Uncommitted changes ###\n");
        content.push_str(&uncommitted_diff);
    }

    // Write to a temp file then rename for atomicity (prevents truncated files on crash)
    let temp_path = patch_path.with_extension("patch.tmp");
    std::fs::write(&temp_path, &content)
        .with_context(|| format!("failed to write snapshot temp file {}", temp_path.display()))?;
    std::fs::rename(&temp_path, &patch_path)
        .with_context(|| format!("failed to rename snapshot to {}", patch_path.display()))?;

    Ok(SnapshotEntry {
        name: worktree.name.clone(),
        patch_file: patch_path,
        base_commit: worktree.base_commit.clone(),
        base_branch: worktree.base_branch.clone(),
        deleted_at: Utc::now(),
    })
}

/// List existing snapshots from disk.
pub fn list_snapshots() -> Result<Vec<PathBuf>> {
    let dir = snapshot_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut snapshots: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "patch"))
        .collect();

    snapshots.sort();
    Ok(snapshots)
}
