use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::worktree::model::WorktreeStatus;

/// Determine the session status for a worktree based on its tmux pane or background PID.
/// Handles both tmux pane IDs (e.g., "%5") and headless PIDs (e.g., "pid:12345").
pub fn check_status(tmux_pane: Option<&str>) -> WorktreeStatus {
    match tmux_pane {
        None => WorktreeStatus::Idle,
        Some(id) if id.starts_with("pid:") => {
            // Headless background process — check if PID is still alive
            match id.strip_prefix("pid:").and_then(|s| s.parse::<u32>().ok()) {
                Some(pid) => {
                    if is_pid_alive(pid) {
                        WorktreeStatus::Running
                    } else {
                        WorktreeStatus::Done
                    }
                }
                None => WorktreeStatus::Done,
            }
        }
        Some(pane_id) => {
            // tmux pane — single atomic query to avoid TOCTOU race
            match crate::tmux::pane::pane_current_command(pane_id) {
                Ok(cmd) => {
                    let cmd_lower = cmd.to_lowercase();
                    if cmd_lower.contains("claude") {
                        WorktreeStatus::Running
                    } else {
                        // Pane exists but claude isn't the foreground process — session ended
                        WorktreeStatus::Done
                    }
                }
                // Pane doesn't exist or tmux error
                Err(_) => WorktreeStatus::Done,
            }
        }
    }
}

/// Check if a process with the given PID is still running.
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) checks if the process exists without sending a signal.
        // SAFETY: signal 0 does not affect the target process.
        unsafe { libc_kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    // Use direct syscall via libc-style FFI
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, sig) }
}

/// Find Claude Code project directory for a given worktree path.
/// Claude stores sessions at `~/.claude/projects/<encoded-path>/`
/// where the encoded path replaces `/` with `-` from the absolute path.
pub fn find_project_dir(worktree_path: &Path) -> Result<Option<PathBuf>> {
    let claude_dir = match dirs::home_dir() {
        Some(home) => home.join(".claude").join("projects"),
        None => return Ok(None),
    };

    if !claude_dir.exists() {
        return Ok(None);
    }

    let abs_path =
        std::fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
    let path_str = abs_path.to_string_lossy();

    // Claude Code encodes project paths by replacing '/' with '-' and stripping the leading slash.
    // e.g., /home/user/project -> home-user-project
    let encoded = path_str
        .strip_prefix('/')
        .unwrap_or(&path_str)
        .replace('/', "-");

    // Try exact match first
    let exact = claude_dir.join(&encoded);
    if exact.is_dir() {
        return Ok(Some(exact));
    }

    // Fallback: heuristic search for partial matches
    let last_component = abs_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Build path suffixes to match against (handles symlinks, mount points, etc.)
    let path_parts: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();
    let suffix_2 = if path_parts.len() >= 2 {
        format!(
            "{}-{}",
            path_parts[path_parts.len() - 2],
            path_parts[path_parts.len() - 1]
        )
    } else {
        String::new()
    };

    let mut best_match: Option<(PathBuf, std::time::SystemTime)> = None;

    for entry in std::fs::read_dir(&claude_dir)? {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let dir_name = entry.file_name();
        let dir_name_str = dir_name.to_string_lossy();

        let matches = dir_name_str == encoded
            || dir_name_str.ends_with(&format!("-{}", last_component))
            || (!suffix_2.is_empty() && dir_name_str.ends_with(&suffix_2));

        if matches {
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);

            if best_match.as_ref().map(|(_, t)| mtime > *t).unwrap_or(true) {
                best_match = Some((entry.path(), mtime));
            }
        }
    }

    Ok(best_match.map(|(p, _)| p))
}

/// Find the most recent session ID from a Claude project directory.
/// Session files are `.jsonl` files named with the session ID.
pub fn find_latest_session_id(project_dir: &Path) -> Result<Option<String>> {
    let mut jsonl_files: Vec<_> = std::fs::read_dir(project_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();

    jsonl_files.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    Ok(jsonl_files
        .first()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string()))
}
