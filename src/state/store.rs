use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::worktree::model::Worktree;

/// Snapshot metadata persisted in state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub name: String,
    pub patch_file: PathBuf,
    pub base_commit: String,
    pub base_branch: String,
    pub deleted_at: DateTime<Utc>,
}

/// Top-level state persisted to .cwt/state.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub repo_root: PathBuf,
    pub worktrees: HashMap<String, Worktree>,
    #[serde(default)]
    pub snapshots: Vec<SnapshotEntry>,
}

impl State {
    pub fn new(repo_root: PathBuf) -> Self {
        Self {
            version: 1,
            repo_root,
            worktrees: HashMap::new(),
            snapshots: Vec::new(),
        }
    }
}

/// The state store, responsible for loading and saving .cwt/state.json.
pub struct StateStore {
    path: PathBuf,
}

impl StateStore {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            path: repo_root.join(".cwt/state.json"),
        }
    }

    /// Load state from disk, or create a new one if it doesn't exist.
    pub fn load(&self, repo_root: &Path) -> Result<State> {
        if self.path.exists() {
            let content = std::fs::read_to_string(&self.path)
                .with_context(|| format!("failed to read {}", self.path.display()))?;
            let state: State = serde_json::from_str(&content)
                .with_context(|| format!("failed to parse {}", self.path.display()))?;
            Ok(state)
        } else {
            Ok(State::new(repo_root.to_path_buf()))
        }
    }

    /// Save state to disk.
    pub fn save(&self, state: &State) -> Result<()> {
        // Ensure .cwt directory exists
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(state).context("failed to serialize state")?;
        std::fs::write(&self.path, content)
            .with_context(|| format!("failed to write {}", self.path.display()))?;
        Ok(())
    }

    /// Merge state with actual git worktree list to handle drift.
    /// Removes state entries for worktrees that no longer exist on disk,
    /// but does NOT add entries for worktrees not managed by cwt.
    pub fn reconcile(
        &self,
        state: &mut State,
        git_worktrees: &[crate::git::commands::GitWorktree],
    ) {
        let git_paths: std::collections::HashSet<PathBuf> =
            git_worktrees.iter().map(|w| w.path.clone()).collect();

        // Remove entries whose paths no longer exist in git worktree list
        // Note: remote worktrees don't have local git paths, so always retain them
        state.worktrees.retain(|_name, wt| {
            // Remote worktrees are not in the local git worktree list
            if wt.remote_host.is_some() {
                return true;
            }
            let abs_path = if wt.path.is_relative() {
                state.repo_root.join(&wt.path)
            } else {
                wt.path.clone()
            };
            git_paths.contains(&abs_path)
        });
    }
}
