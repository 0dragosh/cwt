use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config;
use crate::session;
use crate::worktree::Manager;
use crate::worktree::model::WorktreeStatus;

/// Per-repo statistics tracked in the global index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoStats {
    pub worktree_count: usize,
    pub running_sessions: usize,
    pub waiting_sessions: usize,
    pub done_sessions: usize,
    pub last_updated: Option<DateTime<Utc>>,
}

/// A repo entry in the global index (path + cached stats).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexRepoEntry {
    pub path: PathBuf,
    pub name: String,
    pub stats: RepoStats,
}

/// The global index stored at ~/.config/cwt/index.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalIndex {
    pub version: u32,
    pub repos: HashMap<String, IndexRepoEntry>,
}

/// Return the path to ~/.config/cwt/index.json.
pub fn index_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cwt").join("index.json"))
}

/// Load the global index. Returns empty index if file doesn't exist.
pub fn load_index() -> Result<GlobalIndex> {
    let path = match index_path() {
        Some(p) => p,
        None => return Ok(GlobalIndex::default()),
    };

    if !path.exists() {
        return Ok(GlobalIndex { version: 1, repos: HashMap::new() });
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let index: GlobalIndex = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(index)
}

/// Save the global index.
pub fn save_index(index: &GlobalIndex) -> Result<()> {
    let path = index_path()
        .context("unable to determine config directory")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let content = serde_json::to_string_pretty(index)
        .context("failed to serialize global index")?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Compute live stats for a single repo by loading its state and checking session statuses.
pub fn compute_repo_stats(repo_root: &Path) -> RepoStats {
    let cfg = config::load_config(repo_root).unwrap_or_default();
    let manager = Manager::new(repo_root.to_path_buf(), cfg);

    let worktrees = match manager.list() {
        Ok(wts) => wts,
        Err(_) => return RepoStats::default(),
    };

    let mut stats = RepoStats {
        worktree_count: worktrees.len(),
        running_sessions: 0,
        waiting_sessions: 0,
        done_sessions: 0,
        last_updated: Some(Utc::now()),
    };

    for wt in &worktrees {
        let live_status = session::tracker::check_status(wt.tmux_pane.as_deref());
        match live_status {
            WorktreeStatus::Running => stats.running_sessions += 1,
            WorktreeStatus::Waiting => stats.waiting_sessions += 1,
            WorktreeStatus::Done => stats.done_sessions += 1,
            WorktreeStatus::Idle | WorktreeStatus::Shipping => {}
        }
    }

    stats
}

/// Refresh the global index by scanning all registered repos.
pub fn refresh_index(forest_config: &crate::forest::ForestConfig) -> Result<GlobalIndex> {
    let mut index = GlobalIndex {
        version: 1,
        repos: HashMap::new(),
    };

    for repo in &forest_config.repo {
        let stats = compute_repo_stats(&repo.path);
        index.repos.insert(
            repo.name.clone(),
            IndexRepoEntry {
                path: repo.path.clone(),
                name: repo.name.clone(),
                stats,
            },
        );
    }

    save_index(&index)?;
    Ok(index)
}

/// Get aggregate stats across all repos in the index.
pub fn aggregate_stats(index: &GlobalIndex) -> (usize, usize, usize, usize, usize) {
    let repo_count = index.repos.len();
    let mut total_worktrees = 0;
    let mut total_running = 0;
    let mut total_waiting = 0;
    let mut total_done = 0;

    for entry in index.repos.values() {
        total_worktrees += entry.stats.worktree_count;
        total_running += entry.stats.running_sessions;
        total_waiting += entry.stats.waiting_sessions;
        total_done += entry.stats.done_sessions;
    }

    (repo_count, total_worktrees, total_running, total_waiting, total_done)
}
