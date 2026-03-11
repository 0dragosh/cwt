use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Information about a tmux pane.
#[derive(Debug, Clone)]
pub struct PaneInfo {
    pub pane_id: String,
    pub pane_title: String,
    pub current_command: String,
    pub is_active: bool,
}

/// Check if we're currently running inside a tmux session.
pub fn is_inside_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Get the current tmux session name.
pub fn current_session() -> Result<String> {
    let output = tmux(&["display-message", "-p", "#{session_name}"])?;
    Ok(output.trim().to_string())
}

/// Create a new tmux pane and run a command in it.
/// Returns the pane ID (e.g., "%5").
pub fn create_pane(worktree_path: &Path, command: &str, pane_title: &str) -> Result<String> {
    let path_str = worktree_path
        .to_str()
        .context("worktree path is not valid UTF-8")?;

    let shell_cmd = format!("cd {} && {}", shell_escape(path_str), command);

    let output = tmux(&[
        "split-window",
        "-h",
        "-d", // don't switch focus yet
        "-P",
        "-F",
        "#{pane_id}",
        &shell_cmd,
    ])?;

    let pane_id = output.trim().to_string();

    // Set the pane title
    let _ = tmux(&["select-pane", "-t", &pane_id, "-T", pane_title]);

    Ok(pane_id)
}

/// Focus (select) an existing tmux pane.
pub fn focus_pane(pane_id: &str) -> Result<()> {
    tmux(&["select-pane", "-t", pane_id])?;
    Ok(())
}

/// Kill a tmux pane.
pub fn kill_pane(pane_id: &str) -> Result<()> {
    tmux(&["kill-pane", "-t", pane_id])?;
    Ok(())
}

/// Check if a pane is still alive by querying its pane_id.
pub fn pane_exists(pane_id: &str) -> bool {
    Command::new("tmux")
        .args(["display-message", "-t", pane_id, "-p", "#{pane_id}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the current command running in a pane.
pub fn pane_current_command(pane_id: &str) -> Result<String> {
    let output = tmux(&[
        "display-message",
        "-t",
        pane_id,
        "-p",
        "#{pane_current_command}",
    ])?;
    Ok(output.trim().to_string())
}

/// List all panes in the current tmux session with their info.
pub fn list_panes() -> Result<Vec<PaneInfo>> {
    let output = tmux(&[
        "list-panes",
        "-s", // all panes in session
        "-F",
        "#{pane_id}\t#{pane_title}\t#{pane_current_command}\t#{pane_active}",
    ])?;

    let mut panes = Vec::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 4 {
            panes.push(PaneInfo {
                pane_id: parts[0].to_string(),
                pane_title: parts[1].to_string(),
                current_command: parts[2].to_string(),
                is_active: parts[3] == "1",
            });
        }
    }

    Ok(panes)
}

/// Get the PID of the process running in a pane.
pub fn pane_pid(pane_id: &str) -> Result<u32> {
    let output = tmux(&["display-message", "-t", pane_id, "-p", "#{pane_pid}"])?;
    let pid: u32 = output
        .trim()
        .parse()
        .with_context(|| format!("invalid pid for pane {}", pane_id))?;
    Ok(pid)
}

/// Send keys to a pane (e.g., for sending input).
pub fn send_keys(pane_id: &str, keys: &str) -> Result<()> {
    tmux(&["send-keys", "-t", pane_id, keys, "Enter"])?;
    Ok(())
}

/// Run a tmux command and return stdout.
fn tmux(args: &[&str]) -> Result<String> {
    let output = Command::new("tmux")
        .args(args)
        .output()
        .with_context(|| format!("failed to run tmux {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Simple shell escaping for paths.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
