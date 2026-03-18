use anyhow::{Context, Result};
use std::path::Path;

use crate::config::model::{PermissionLevel, SessionConfig};
use crate::session::launcher::inject_settings_override;
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
    permission: PermissionLevel,
) -> Vec<DispatchResult> {
    tasks
        .iter()
        .map(|task| dispatch_one(manager, task, base_branch, permission))
        .collect()
}

/// Dispatch a single task: create worktree, launch Claude with --prompt.
fn dispatch_one(
    manager: &Manager,
    task: &str,
    base_branch: &str,
    permission: PermissionLevel,
) -> DispatchResult {
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
    let pane_id = match launch_with_prompt(&wt, &wt_abs, task, &manager.config.session, permission)
    {
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

    // Update state with pane ID, running status, and task info
    if let Ok(mut state) = manager.load_state() {
        if let Some(stored) = state.worktrees.get_mut(&wt.name) {
            stored.tmux_pane = Some(pane_id.clone());
            stored.status = WorktreeStatus::Running;
            stored.task_title = Some(task.to_string());
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

/// Launch a provider session with an initial prompt using -p flag.
pub fn launch_with_prompt(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    prompt: &str,
    config: &SessionConfig,
    permission: PermissionLevel,
) -> Result<String> {
    if !tmux::pane::is_inside_tmux() {
        anyhow::bail!(
            "cwt sessions require an active terminal multiplexer (zellij preferred, tmux fallback)"
        );
    }

    if config.provider == crate::session::provider::SessionProvider::Claude {
        if let Some(ref settings) = config.permissions.get(permission).settings_override {
            inject_settings_override(worktree_abs_path, settings)?;
        }
    }

    let command = config.provider.resolve_command(&config.command);

    let mut cmd_parts = vec![command];
    // Add the prompt flag
    cmd_parts.push("-p".to_string());
    cmd_parts.push(shell_quote(prompt));
    for arg in &config.provider_args {
        cmd_parts.push(shell_quote(arg));
    }
    let permission_args: Vec<String> =
        if config.provider == crate::session::provider::SessionProvider::Codex {
            config
                .provider
                .permission_args(permission)
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            config.permissions.get(permission).extra_args.clone()
        };
    for arg in permission_args {
        cmd_parts.push(arg);
    }
    let command = cmd_parts.join(" ");

    let pane_title = format!("cwt:{}", worktree.name);

    let pane_id = tmux::pane::create_pane(worktree_abs_path, &command, &pane_title)
        .with_context(|| format!("failed to launch session for '{}'", worktree.name))?;

    Ok(pane_id)
}

/// Shell-quote a string for safe embedding in a tmux command.
/// Single-quoting prevents expansion of $, `, !, etc.
/// Newlines are replaced with spaces to prevent command splitting.
fn shell_quote(s: &str) -> String {
    let sanitized = s.replace(['\n', '\r'], " ");
    format!("'{}'", sanitized.replace('\'', "'\\''"))
}
