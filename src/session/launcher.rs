use anyhow::{Context, Result};
use std::path::Path;

use crate::config::model::SessionConfig;
use crate::tmux;
use crate::worktree::model::Worktree;

/// Launch a new Claude Code session in a tmux pane for the given worktree.
/// Returns the tmux pane ID.
pub fn launch_session(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    config: &SessionConfig,
) -> Result<String> {
    if !tmux::pane::is_inside_tmux() {
        anyhow::bail!("cwt sessions require tmux — please run cwt inside a tmux session");
    }

    let mut cmd_parts = vec!["claude".to_string()];
    for arg in &config.claude_args {
        cmd_parts.push(arg.clone());
    }
    let command = cmd_parts.join(" ");

    let pane_title = format!("cwt:{}", worktree.name);

    let pane_id = tmux::pane::create_pane(worktree_abs_path, &command, &pane_title)
        .with_context(|| format!("failed to launch session for '{}'", worktree.name))?;

    Ok(pane_id)
}

/// Resume a previous Claude Code session in a new tmux pane.
/// Uses `claude --resume` to continue the conversation.
/// Returns the tmux pane ID.
pub fn resume_session(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    session_id: &str,
    config: &SessionConfig,
) -> Result<String> {
    if !tmux::pane::is_inside_tmux() {
        anyhow::bail!("cwt sessions require tmux — please run cwt inside a tmux session");
    }

    let mut cmd_parts = vec!["claude".to_string(), "--resume".to_string(), session_id.to_string()];
    for arg in &config.claude_args {
        cmd_parts.push(arg.clone());
    }
    let command = cmd_parts.join(" ");

    let pane_title = format!("cwt:{}", worktree.name);

    let pane_id = tmux::pane::create_pane(worktree_abs_path, &command, &pane_title)
        .with_context(|| format!("failed to resume session for '{}'", worktree.name))?;

    Ok(pane_id)
}

/// Focus an existing session pane.
pub fn focus_session(pane_id: &str) -> Result<()> {
    tmux::pane::focus_pane(pane_id)
}

/// Check if a session pane is still alive.
pub fn is_session_alive(pane_id: &str) -> bool {
    tmux::pane::pane_exists(pane_id)
}

/// Kill a session pane.
pub fn kill_session(pane_id: &str) -> Result<()> {
    tmux::pane::kill_pane(pane_id)
}
