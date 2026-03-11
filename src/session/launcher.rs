use anyhow::{Context, Result};
use std::path::Path;

use crate::config::model::SessionConfig;
use crate::env::container::{ContainerRuntime, ContainerStatus};
use crate::tmux;
use crate::worktree::model::Worktree;

/// Launch a new Claude Code session in a tmux pane for the given worktree.
/// If the worktree has a running container, the session runs inside it.
/// Returns the tmux pane ID.
pub fn launch_session(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    config: &SessionConfig,
) -> Result<String> {
    if !tmux::pane::is_inside_tmux() {
        anyhow::bail!("cwt sessions require tmux — please run cwt inside a tmux session");
    }

    let command = build_claude_command(worktree, config, None);
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

    let command = build_claude_command(worktree, config, Some(session_id));
    let pane_title = format!("cwt:{}", worktree.name);

    let pane_id = tmux::pane::create_pane(worktree_abs_path, &command, &pane_title)
        .with_context(|| format!("failed to resume session for '{}'", worktree.name))?;

    Ok(pane_id)
}

/// Build the claude command string, optionally wrapping it in a container exec.
fn build_claude_command(
    worktree: &Worktree,
    config: &SessionConfig,
    resume_session_id: Option<&str>,
) -> String {
    let mut cmd_parts = vec!["claude".to_string()];

    if let Some(sid) = resume_session_id {
        cmd_parts.push("--resume".to_string());
        cmd_parts.push(sid.to_string());
    }

    for arg in &config.claude_args {
        cmd_parts.push(arg.clone());
    }

    let claude_cmd = cmd_parts.join(" ");

    // If the worktree has a running container, exec into it
    if let Some(ref container) = worktree.container {
        if container.status == ContainerStatus::Running {
            if let Some(ref cid) = container.container_id {
                return build_container_exec_command(&container.runtime, cid, &claude_cmd);
            }
            if let Some(ref name) = container.container_name {
                return build_container_exec_command(&container.runtime, name, &claude_cmd);
            }
        }
    }

    claude_cmd
}

/// Build a container exec command that runs claude inside the container.
fn build_container_exec_command(
    runtime: &ContainerRuntime,
    container_id: &str,
    inner_command: &str,
) -> String {
    format!(
        "{} exec -it -w /workspace {} sh -c '{}'",
        runtime.cmd(),
        container_id,
        inner_command.replace('\'', "'\\''"),
    )
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
