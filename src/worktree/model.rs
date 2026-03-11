use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Lifecycle of a worktree: ephemeral (auto-GC'd) or permanent (never auto-deleted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lifecycle {
    Ephemeral,
    Permanent,
}

/// Runtime status of a worktree's session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeStatus {
    #[default]
    Idle,
    Running,
    Waiting,
    Done,
}

/// A managed worktree with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    pub base_branch: String,
    pub base_commit: String,
    pub lifecycle: Lifecycle,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_session_id: Option<String>,
    #[serde(default)]
    pub tmux_pane: Option<String>,
    #[serde(default)]
    pub status: WorktreeStatus,
}

impl Worktree {
    pub fn new(
        name: String,
        path: PathBuf,
        branch: String,
        base_branch: String,
        base_commit: String,
        lifecycle: Lifecycle,
    ) -> Self {
        Self {
            name,
            path,
            branch,
            base_branch,
            base_commit,
            lifecycle,
            created_at: Utc::now(),
            last_session_id: None,
            tmux_pane: None,
            status: WorktreeStatus::Idle,
        }
    }

    pub fn is_ephemeral(&self) -> bool {
        self.lifecycle == Lifecycle::Ephemeral
    }
}
