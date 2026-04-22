use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::session::provider::SessionProvider;
use crate::worktree::model::WorktreeStatus;

/// Determine the session status for a worktree based on its tmux pane.
/// Uses a single tmux query to avoid TOCTOU race between pane_exists and pane_current_command.
pub fn check_status(tmux_pane: Option<&str>) -> WorktreeStatus {
    match tmux_pane {
        None => WorktreeStatus::Idle,
        Some(pane_id) => {
            // Single atomic query: if the pane exists, this returns the command;
            // if it doesn't, the command fails.
            match crate::tmux::pane::pane_current_command(pane_id) {
                Ok(cmd) if SessionProvider::matches_any_process(&cmd) => WorktreeStatus::Running,
                Ok(_) => {
                    // Pane exists but provider CLI isn't the foreground process — session ended
                    WorktreeStatus::Done
                }
                // Pane doesn't exist or tmux error
                Err(_) => WorktreeStatus::Done,
            }
        }
    }
}

/// Find the provider-specific session directory for a given worktree path.
pub fn find_project_dir(provider: SessionProvider, worktree_path: &Path) -> Result<Option<PathBuf>> {
    let home_dir = match dirs::home_dir() {
        Some(home) => home,
        None => return Ok(None),
    };
    find_project_dir_with_home(provider, worktree_path, &home_dir)
}

/// Find the most recent session ID from a provider session directory.
/// Session files are `.jsonl` files named with the session ID.
pub fn find_latest_session_id(
    _provider: SessionProvider,
    project_dir: &Path,
) -> Result<Option<String>> {
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

fn find_project_dir_with_home(
    provider: SessionProvider,
    worktree_path: &Path,
    home_dir: &Path,
) -> Result<Option<PathBuf>> {
    let session_root = provider_session_root(provider, home_dir);
    if !session_root.exists() {
        return Ok(None);
    }

    let abs_path =
        std::fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
    let path_str = abs_path.to_string_lossy();
    let encoded_core = encode_path_component(&path_str);
    let encoded_dir = provider_dir_name(provider, &encoded_core);

    // Try exact match first.
    let exact = session_root.join(&encoded_dir);
    if exact.is_dir() {
        return Ok(Some(exact));
    }

    // Fallback: heuristic search for partial matches. This preserves the old
    // Claude/Codex behavior and gives Pi the same resilience to symlinked or
    // remapped working directories.
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

    for entry in std::fs::read_dir(&session_root)? {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let dir_name = entry.file_name();
        let dir_name_str = dir_name.to_string_lossy();
        let normalized_name = normalize_provider_dir_name(provider, &dir_name_str);

        let matches = dir_name_str == encoded_dir
            || normalized_name == encoded_core
            || normalized_name.ends_with(&format!("-{}", last_component))
            || (!suffix_2.is_empty() && normalized_name.ends_with(&suffix_2));

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

fn provider_session_root(provider: SessionProvider, home_dir: &Path) -> PathBuf {
    match provider {
        SessionProvider::Claude | SessionProvider::Codex => home_dir.join(".claude").join("projects"),
        SessionProvider::Pi => home_dir.join(".pi").join("agent").join("sessions"),
    }
}

fn encode_path_component(path: &str) -> String {
    path.strip_prefix('/').unwrap_or(path).replace('/', "-")
}

fn provider_dir_name(provider: SessionProvider, encoded_core: &str) -> String {
    match provider {
        SessionProvider::Claude | SessionProvider::Codex => encoded_core.to_string(),
        SessionProvider::Pi => format!("--{}--", encoded_core),
    }
}

fn normalize_provider_dir_name<'a>(provider: SessionProvider, dir_name: &'a str) -> &'a str {
    match provider {
        SessionProvider::Claude | SessionProvider::Codex => dir_name,
        SessionProvider::Pi => dir_name
            .strip_prefix("--")
            .and_then(|name| name.strip_suffix("--"))
            .unwrap_or(dir_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn make_worktree(temp: &TempDir) -> PathBuf {
        let path = temp.path().join("repo").join("worktree");
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn pi_project_dir_uses_provider_specific_encoding() {
        let home = tempfile::tempdir().unwrap();
        let worktree = make_worktree(&home);
        let canonical = std::fs::canonicalize(&worktree).unwrap();
        let encoded = provider_dir_name(
            SessionProvider::Pi,
            &encode_path_component(&canonical.to_string_lossy()),
        );
        let expected = home
            .path()
            .join(".pi/agent/sessions")
            .join(&encoded);
        std::fs::create_dir_all(&expected).unwrap();

        let found = find_project_dir_with_home(SessionProvider::Pi, &worktree, home.path())
            .unwrap()
            .unwrap();
        assert_eq!(found, expected);
    }

    #[test]
    fn claude_and_codex_share_claude_project_root() {
        let home = tempfile::tempdir().unwrap();
        let worktree = make_worktree(&home);
        let canonical = std::fs::canonicalize(&worktree).unwrap();
        let encoded = provider_dir_name(
            SessionProvider::Claude,
            &encode_path_component(&canonical.to_string_lossy()),
        );
        let expected = home
            .path()
            .join(".claude/projects")
            .join(&encoded);
        std::fs::create_dir_all(&expected).unwrap();

        let claude = find_project_dir_with_home(SessionProvider::Claude, &worktree, home.path())
            .unwrap()
            .unwrap();
        let codex = find_project_dir_with_home(SessionProvider::Codex, &worktree, home.path())
            .unwrap()
            .unwrap();

        assert_eq!(claude, expected);
        assert_eq!(codex, expected);
    }

    #[test]
    fn latest_session_id_picks_newest_pi_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let older = dir.path().join("2026-04-21_old.jsonl");
        let newer = dir.path().join("2026-04-22_new.jsonl");
        std::fs::write(&older, "").unwrap();
        std::thread::sleep(Duration::from_millis(15));
        std::fs::write(&newer, "").unwrap();

        let latest = find_latest_session_id(SessionProvider::Pi, dir.path()).unwrap();
        assert_eq!(latest.as_deref(), Some("2026-04-22_new"));
    }

    #[test]
    fn latest_session_id_ignores_non_jsonl_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "").unwrap();
        std::fs::write(dir.path().join("sess-123.jsonl"), "").unwrap();

        let latest = find_latest_session_id(SessionProvider::Claude, dir.path()).unwrap();
        assert_eq!(latest.as_deref(), Some("sess-123"));
    }
}
