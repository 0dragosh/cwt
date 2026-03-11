use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::config::Config;
use crate::git;
use crate::state::{State, StateStore};
use crate::worktree::model::{Lifecycle, Worktree, WorktreeStatus};
use crate::worktree::slug::generate_slug;
use crate::worktree::snapshot;

/// High-level worktree manager that ties git, state, and config together.
pub struct Manager {
    pub repo_root: PathBuf,
    pub config: Config,
    store: StateStore,
}

impl Manager {
    pub fn new(repo_root: PathBuf, config: Config) -> Self {
        let store = StateStore::new(&repo_root);
        Self {
            repo_root,
            config,
            store,
        }
    }

    /// Load and reconcile state.
    pub fn load_state(&self) -> Result<State> {
        let mut state = self.store.load(&self.repo_root)?;
        let git_wts = git::commands::worktree_list(&self.repo_root)?;
        self.store.reconcile(&mut state, &git_wts);
        Ok(state)
    }

    /// Save state to disk.
    pub fn save_state(&self, state: &State) -> Result<()> {
        self.store.save(state)
    }

    /// List all managed worktrees, merged with git data.
    pub fn list(&self) -> Result<Vec<Worktree>> {
        let state = self.load_state()?;
        let mut worktrees: Vec<Worktree> = state.worktrees.values().cloned().collect();
        worktrees.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(worktrees)
    }

    /// Create a new worktree.
    pub fn create(
        &self,
        name: Option<&str>,
        base_branch: &str,
        carry_changes: bool,
    ) -> Result<Worktree> {
        let name = match name {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => generate_slug(),
        };

        let wt_dir = &self.config.worktree.dir;
        let wt_rel_path = PathBuf::from(wt_dir).join(&name);
        let wt_abs_path = self.repo_root.join(&wt_rel_path);
        let branch_name = format!("wt/{}", name);

        // Get base commit
        let base_commit = git::commands::head_commit(&self.repo_root)?;

        // Handle carrying local changes
        let stashed = if carry_changes && git::commands::is_dirty(&self.repo_root)? {
            git::commands::stash(&self.repo_root)?
        } else {
            false
        };

        // Create the worktree
        git::commands::worktree_add(&self.repo_root, &wt_abs_path, &branch_name, base_branch)
            .with_context(|| format!("failed to create worktree '{}'", name))?;

        // Apply stashed changes to the new worktree
        if stashed {
            // Apply stash to worktree (best-effort)
            let stash_diff = std::process::Command::new("git")
                .args(["stash", "show", "-p"])
                .current_dir(&self.repo_root)
                .output()
                .context("failed to get stash diff")?;

            if stash_diff.status.success() {
                let patch = String::from_utf8_lossy(&stash_diff.stdout);
                if !patch.is_empty() {
                    let _ = git::commands::apply_patch(&wt_abs_path, &patch);
                }
            }

            // Pop stash in original dir to restore it
            let _ = git::commands::stash_pop(&self.repo_root);
        }

        let worktree = Worktree::new(
            name.clone(),
            wt_rel_path,
            branch_name,
            base_branch.to_string(),
            base_commit,
            Lifecycle::Ephemeral,
        );

        // Update state
        let mut state = self.load_state()?;
        state.worktrees.insert(name, worktree.clone());
        self.save_state(&state)?;

        Ok(worktree)
    }

    /// Delete a worktree, saving a snapshot first.
    pub fn delete(&self, name: &str) -> Result<()> {
        let mut state = self.load_state()?;

        let worktree = state
            .worktrees
            .get(name)
            .with_context(|| format!("worktree '{}' not found", name))?
            .clone();

        // Save snapshot
        let snap = snapshot::save_snapshot(&worktree, &self.repo_root)?;
        state.snapshots.push(snap);

        // Remove worktree from git
        let wt_abs_path = if worktree.path.is_relative() {
            self.repo_root.join(&worktree.path)
        } else {
            worktree.path.clone()
        };

        git::commands::worktree_remove(&self.repo_root, &wt_abs_path, true)
            .with_context(|| format!("failed to remove worktree '{}'", name))?;

        // Delete branch
        let _ = git::commands::branch_delete(&self.repo_root, &worktree.branch, true);

        // Remove from state
        state.worktrees.remove(name);
        self.save_state(&state)?;

        Ok(())
    }

    /// Promote an ephemeral worktree to permanent.
    pub fn promote(&self, name: &str) -> Result<()> {
        let mut state = self.load_state()?;

        let worktree = state
            .worktrees
            .get_mut(name)
            .with_context(|| format!("worktree '{}' not found", name))?;

        worktree.lifecycle = Lifecycle::Permanent;
        self.save_state(&state)?;

        Ok(())
    }

    /// Preview what GC would prune: returns list of worktree names that would be deleted.
    pub fn gc_preview(&self) -> Result<Vec<String>> {
        let state = self.load_state()?;

        let mut ephemerals: Vec<&Worktree> = state
            .worktrees
            .values()
            .filter(|wt| wt.is_ephemeral())
            .collect();

        let max = self.config.worktree.max_ephemeral;
        if ephemerals.len() <= max {
            return Ok(Vec::new());
        }

        // Sort by created_at ascending (oldest first)
        ephemerals.sort_by_key(|wt| wt.created_at);

        let to_prune = ephemerals.len() - max;
        let mut prune_names = Vec::new();

        for wt in ephemerals.into_iter().take(to_prune) {
            let wt_abs_path = if wt.path.is_relative() {
                self.repo_root.join(&wt.path)
            } else {
                wt.path.clone()
            };

            // Skip worktrees with running sessions
            if wt.status == WorktreeStatus::Running {
                continue;
            }

            // Skip worktrees with uncommitted changes
            if let Ok(dirty) = git::commands::is_dirty(&wt_abs_path) {
                if dirty {
                    continue;
                }
            }

            // Skip worktrees with unpushed commits
            if let Ok(unpushed) = git::commands::has_unpushed_commits(&wt_abs_path, &wt.branch) {
                if unpushed {
                    continue;
                }
            }

            prune_names.push(wt.name.clone());
        }

        Ok(prune_names)
    }

    /// Execute GC: snapshot and delete the given worktrees.
    pub fn gc_execute(&self, names: &[String]) -> Result<Vec<String>> {
        let mut deleted = Vec::new();
        for name in names {
            match self.delete(name) {
                Ok(()) => deleted.push(name.clone()),
                Err(e) => eprintln!("warning: failed to GC '{}': {}", name, e),
            }
        }
        Ok(deleted)
    }

    /// Resolve the absolute path of a worktree.
    pub fn worktree_abs_path(&self, worktree: &Worktree) -> PathBuf {
        if worktree.path.is_relative() {
            self.repo_root.join(&worktree.path)
        } else {
            worktree.path.clone()
        }
    }
}
