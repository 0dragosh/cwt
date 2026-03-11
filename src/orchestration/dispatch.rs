use anyhow::{Context, Result};
use std::path::Path;

use crate::config::model::SessionConfig;
use crate::tmux;
use crate::worktree::model::{Worktree, WorktreeStatus};
use crate::worktree::Manager;

/// Result of dispatching a single task.
#[derive(Debug)]
pub struct DispatchResult {
    pub task: String,
    pub worktree_name: String,
    pub pane_id: Option<String>,
    pub error: Option<String>,
}

/// Dispatch multiple tasks: create a worktree for each and launch Claude with the task as prompt.
/// Returns a result per task.
pub fn dispatch_tasks(
    manager: &Manager,
    tasks: &[String],
    base_branch: &str,
) -> Vec<DispatchResult> {
    tasks
        .iter()
        .map(|task| dispatch_one(manager, task, base_branch))
        .collect()
}

/// Dispatch a single task: create worktree, launch Claude with --prompt.
fn dispatch_one(manager: &Manager, task: &str, base_branch: &str) -> DispatchResult {
    // Create worktree (auto-name)
    let wt = match manager.create(None, base_branch, false) {
        Ok(wt) => wt,
        Err(e) => {
            return DispatchResult {
                task: task.to_string(),
                worktree_name: String::new(),
                pane_id: None,
                error: Some(format!("Failed to create worktree: {}", e)),
            };
        }
    };

    let wt_abs = manager.worktree_abs_path(&wt);

    // Launch Claude with --prompt flag
    let pane_id = match launch_with_prompt(&wt, &wt_abs, task, &manager.config.session) {
        Ok(id) => id,
        Err(e) => {
            return DispatchResult {
                task: task.to_string(),
                worktree_name: wt.name.clone(),
                pane_id: None,
                error: Some(format!("Failed to launch session: {}", e)),
            };
        }
    };

    // Update state with pane ID and running status
    if let Ok(mut state) = manager.load_state() {
        if let Some(stored) = state.worktrees.get_mut(&wt.name) {
            stored.tmux_pane = Some(pane_id.clone());
            stored.status = WorktreeStatus::Running;
        }
        let _ = manager.save_state(&state);
    }

    DispatchResult {
        task: task.to_string(),
        worktree_name: wt.name.clone(),
        pane_id: Some(pane_id),
        error: None,
    }
}

/// Launch a Claude Code session with an initial prompt using -p flag.
pub fn launch_with_prompt(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    prompt: &str,
    config: &SessionConfig,
) -> Result<String> {
    if !tmux::pane::is_inside_tmux() {
        anyhow::bail!("cwt sessions require tmux -- please run cwt inside a tmux session");
    }

    let mut cmd_parts = vec!["claude".to_string()];
    // Add the prompt flag
    cmd_parts.push("-p".to_string());
    cmd_parts.push(shell_quote(prompt));
    for arg in &config.claude_args {
        cmd_parts.push(arg.clone());
    }
    let command = cmd_parts.join(" ");

    let pane_title = format!("cwt:{}", worktree.name);

    let pane_id = tmux::pane::create_pane(worktree_abs_path, &command, &pane_title)
        .with_context(|| format!("failed to launch session for '{}'", worktree.name))?;

    Ok(pane_id)
}

/// Shell-quote a string for safe embedding in a tmux command.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
