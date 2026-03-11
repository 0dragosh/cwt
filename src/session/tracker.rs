use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::worktree::model::WorktreeStatus;

/// Determine the session status for a worktree based on its tmux pane.
pub fn check_status(tmux_pane: Option<&str>) -> WorktreeStatus {
    match tmux_pane {
        None => WorktreeStatus::Idle,
        Some(pane_id) => {
            if !crate::tmux::pane::pane_exists(pane_id) {
                return WorktreeStatus::Done;
            }

            // Check what command is running
            match crate::tmux::pane::pane_current_command(pane_id) {
                Ok(cmd) => {
                    if cmd.contains("claude") {
                        WorktreeStatus::Running
                    } else {
                        // Pane exists but claude isn't the foreground process
                        WorktreeStatus::Done
                    }
                }
                Err(_) => WorktreeStatus::Done,
            }
        }
    }
}

/// Find Claude Code project directory for a given worktree path.
/// Claude stores sessions at ~/.claude/projects/<path-hash>/
pub fn find_project_dir(worktree_path: &Path) -> Result<Option<PathBuf>> {
    let claude_dir = match dirs::home_dir() {
        Some(home) => home.join(".claude/projects"),
        None => return Ok(None),
    };

    if !claude_dir.exists() {
        return Ok(None);
    }

    // Claude hashes the project path — look for directories that might match
    let abs_path = std::fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
    let path_str = abs_path.to_string_lossy();

    // Claude uses a simple path-based directory naming
    // Try to find a matching project directory
    for entry in std::fs::read_dir(&claude_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            // Check if this directory has sessions matching our path
            let dir_name = entry.file_name();
            let dir_name_str = dir_name.to_string_lossy();

            // Claude encodes path by replacing / with - or similar
            // A heuristic: check if the dir name contains part of our path
            let last_component = abs_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if dir_name_str.contains(&last_component) || path_str.contains(&*dir_name_str) {
                return Ok(Some(entry.path()));
            }
        }
    }

    Ok(None)
}
