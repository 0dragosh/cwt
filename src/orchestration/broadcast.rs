use crate::tmux;
use crate::worktree::model::{Worktree, WorktreeStatus};

/// Result of broadcasting a prompt to a single session.
#[derive(Debug)]
pub struct BroadcastResult {
    pub worktree_name: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Broadcast a prompt to all running sessions by sending keystrokes via tmux.
/// This types the prompt text into each session's tmux pane followed by Enter.
pub fn broadcast_prompt(worktrees: &[Worktree], prompt: &str) -> Vec<BroadcastResult> {
    worktrees
        .iter()
        .filter(|wt| wt.status == WorktreeStatus::Running && wt.tmux_pane.is_some())
        .map(|wt| {
            let pane_id = wt.tmux_pane.as_ref().unwrap();

            // Verify pane is still alive
            if !tmux::pane::pane_exists(pane_id) {
                return BroadcastResult {
                    worktree_name: wt.name.clone(),
                    success: false,
                    error: Some("Pane no longer exists".to_string()),
                };
            }

            // Sanitize the prompt: strip control characters that could interfere with tmux
            let sanitized_prompt: String = prompt
                .chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .collect();

            // Send the prompt text to the pane via tmux send-keys
            match tmux::pane::send_keys(pane_id, &sanitized_prompt) {
                Ok(()) => BroadcastResult {
                    worktree_name: wt.name.clone(),
                    success: true,
                    error: None,
                },
                Err(e) => BroadcastResult {
                    worktree_name: wt.name.clone(),
                    success: false,
                    error: Some(format!("{}", e)),
                },
            }
        })
        .collect()
}

/// Count how many sessions are eligible for broadcast (running with a tmux pane).
pub fn broadcast_target_count(worktrees: &[Worktree]) -> usize {
    worktrees
        .iter()
        .filter(|wt| wt.status == WorktreeStatus::Running && wt.tmux_pane.is_some())
        .count()
}
