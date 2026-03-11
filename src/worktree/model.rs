use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::ship::pr::{CiStatus, PrStatus};

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
    Shipping,
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
    /// PR number on GitHub (if a PR has been created).
    #[serde(default)]
    pub pr_number: Option<u64>,
    /// PR URL on GitHub.
    #[serde(default)]
    pub pr_url: Option<String>,
    /// Current PR status (draft/open/approved/merged/closed).
    #[serde(default)]
    pub pr_status: PrStatus,
    /// Current CI/GitHub Actions status.
    #[serde(default)]
    pub ci_status: CiStatus,
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
            pr_number: None,
            pr_url: None,
            pr_status: PrStatus::None,
            ci_status: CiStatus::None,
        }
    }

    pub fn is_ephemeral(&self) -> bool {
        self.lifecycle == Lifecycle::Ephemeral
    }

    /// Whether this worktree has an active PR (not None, not Merged, not Closed).
    pub fn has_active_pr(&self) -> bool {
        matches!(
            self.pr_status,
            PrStatus::Draft | PrStatus::Open | PrStatus::Approved
        )
    }
}
