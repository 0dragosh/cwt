use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::model::{PermissionLevel, SessionConfig};
use crate::session::launcher::inject_settings_override;
use crate::worktree::model::{Worktree, WorktreeStatus};
use crate::worktree::Manager;

/// Result of dispatching a single task.
#[derive(Debug)]
pub struct DispatchResult {
    pub task: String,
    pub worktree_name: String,
    pub pane_id: Option<String>,
    pub error: Option<String>,
}

/// Dispatch multiple tasks: create a worktree for each and launch Claude with the task as prompt.
/// Returns a result per task.
pub fn dispatch_tasks(
    manager: &Manager,
    tasks: &[String],
    base_branch: &str,
    permission: PermissionLevel,
) -> Vec<DispatchResult> {
    tasks
        .iter()
        .map(|task| dispatch_one(manager, task, base_branch, permission))
        .collect()
}

/// Dispatch a single task: create worktree, launch Claude with --prompt.
fn dispatch_one(
    manager: &Manager,
    task: &str,
    base_branch: &str,
    permission: PermissionLevel,
) -> DispatchResult {
    // Create worktree (auto-name)
    let wt = match manager.create(None, base_branch, false) {
        Ok(wt) => wt,
        Err(e) => {
            return DispatchResult {
                task: task.to_string(),
                worktree_name: String::new(),
                pane_id: None,
                error: Some(format!("Failed to create worktree: {}", e)),
            };
        }
    };

    let wt_abs = manager.worktree_abs_path(&wt);

    // Launch Claude with --prompt flag
    let pane_id = match launch_with_prompt(&wt, &wt_abs, task, &manager.config.session, permission)
    {
        Ok(id) => id,
        Err(e) => {
            return DispatchResult {
                task: task.to_string(),
                worktree_name: wt.name.clone(),
                pane_id: None,
                error: Some(format!("Failed to launch session: {}", e)),
            };
        }
    };

    // Update state with pane ID and running status
    if let Ok(mut state) = manager.load_state() {
        if let Some(stored) = state.worktrees.get_mut(&wt.name) {
            stored.tmux_pane = Some(pane_id.clone());
            stored.status = WorktreeStatus::Running;
        }
        let _ = manager.save_state(&state);
    }

    DispatchResult {
        task: task.to_string(),
        worktree_name: wt.name.clone(),
        pane_id: Some(pane_id),
        error: None,
    }
}

/// Launch a Claude Code session with an initial prompt as a headless background process.
/// Uses `-p` (print mode) which doesn't need a terminal, so we avoid creating
/// tmux panes that clutter the session. Output is logged to `.cwt/logs/<name>.log`.
/// Returns a synthetic identifier for tracking (the PID as a string).
pub fn launch_with_prompt(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    prompt: &str,
    config: &SessionConfig,
    permission: PermissionLevel,
) -> Result<String> {
    // Ensure log directory exists
    let logs_dir = worktree_abs_path
        .ancestors()
        .find(|p| p.join(".cwt").is_dir())
        .unwrap_or(worktree_abs_path)
        .join(".cwt/logs");
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("failed to create log dir {}", logs_dir.display()))?;

    // Inject permission-level settings override if configured
    if let Some(ref settings) = config.permissions.get(permission).settings_override {
        inject_settings_override(worktree_abs_path, settings)?;
    }

    let log_file_path = logs_dir.join(format!("{}.log", worktree.name));
    let log_file = std::fs::File::create(&log_file_path)
        .with_context(|| format!("failed to create log file {}", log_file_path.display()))?;
    let err_file = log_file
        .try_clone()
        .context("failed to clone log file handle")?;

    let mut cmd = Command::new(&config.command);
    cmd.arg("-p").arg(prompt);
    for arg in &config.claude_args {
        cmd.arg(arg);
    }
    for arg in &config.permissions.get(permission).extra_args {
        cmd.arg(arg);
    }

    cmd.current_dir(worktree_abs_path)
        .stdin(Stdio::null())
        .stdout(log_file)
        .stderr(err_file);

    // Detach from the parent's process group so the child survives if cwt exits
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn claude for '{}'", worktree.name))?;

    let pid = child.id();
    Ok(format!("pid:{}", pid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{PermissionLevel, PermissionsConfig, SessionConfig};
    use crate::worktree::model::{Lifecycle, Worktree};
    use std::path::PathBuf;

    fn test_worktree(name: &str) -> Worktree {
        Worktree::new(
            name.to_string(),
            PathBuf::from(format!(".claude/worktrees/{}", name)),
            format!("wt/{}", name),
            "main".to_string(),
            "abc123".to_string(),
            Lifecycle::Ephemeral,
        )
    }

    fn test_config(command: &str) -> SessionConfig {
        SessionConfig {
            auto_launch: false,
            command: command.to_string(),
            claude_args: Vec::new(),
            default_permission: PermissionLevel::Normal,
            permissions: PermissionsConfig::default(),
        }
    }

    #[test]
    fn launch_creates_log_dir_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();

        let wt_path = dir.path().join("worktree");
        std::fs::create_dir_all(&wt_path).unwrap();

        let wt = test_worktree("test-wt");
        // Use a command that exits immediately
        let config = test_config("true");

        let result = launch_with_prompt(&wt, &wt_path, "hello", &config, PermissionLevel::Normal);
        assert!(result.is_ok(), "launch failed: {:?}", result.err());

        let log_path = cwt_dir.join("logs/test-wt.log");
        assert!(log_path.exists(), "log file should be created");
    }

    #[test]
    fn launch_returns_pid_prefixed_id() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();

        let wt_path = dir.path().join("worktree");
        std::fs::create_dir_all(&wt_path).unwrap();

        let wt = test_worktree("test-wt");
        let config = test_config("true");

        let id =
            launch_with_prompt(&wt, &wt_path, "hello", &config, PermissionLevel::Normal).unwrap();
        assert!(id.starts_with("pid:"), "expected pid: prefix, got: {}", id);

        let pid_str = id.strip_prefix("pid:").unwrap();
        let pid: u32 = pid_str.parse().expect("pid should be a valid u32");
        assert!(pid > 0);
    }

    #[test]
    fn launch_appends_permission_extra_args() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();

        let wt_path = dir.path().join("worktree");
        std::fs::create_dir_all(&wt_path).unwrap();

        let wt = test_worktree("test-wt");
        // Use `echo` so we can check args in the log
        let config = test_config("echo");

        let id = launch_with_prompt(
            &wt,
            &wt_path,
            "hello",
            &config,
            PermissionLevel::ElevatedUnsandboxed,
        )
        .unwrap();
        assert!(id.starts_with("pid:"));

        // Wait a bit for the process to finish and flush output
        std::thread::sleep(std::time::Duration::from_millis(200));

        let log_content = std::fs::read_to_string(cwt_dir.join("logs/test-wt.log")).unwrap();
        // echo prints its args: "-p hello --dangerously-skip-permissions"
        assert!(
            log_content.contains("--dangerously-skip-permissions"),
            "log should contain permission arg, got: {}",
            log_content
        );
    }

    #[test]
    fn launch_injects_settings_override_for_elevated() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();

        let wt_path = dir.path().join("worktree");
        std::fs::create_dir_all(&wt_path).unwrap();

        let wt = test_worktree("test-wt");
        let config = test_config("true");

        let _ =
            launch_with_prompt(&wt, &wt_path, "hello", &config, PermissionLevel::Elevated).unwrap();

        // Elevated should have injected settings.local.json
        let settings_path = wt_path.join(".claude/settings.local.json");
        assert!(
            settings_path.exists(),
            "settings.local.json should be created for elevated permission"
        );

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(content["sandbox"]["enabled"], true);
    }

    #[test]
    fn launch_skips_settings_override_for_normal() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();

        let wt_path = dir.path().join("worktree");
        std::fs::create_dir_all(&wt_path).unwrap();

        let wt = test_worktree("test-wt");
        let config = test_config("true");

        let _ =
            launch_with_prompt(&wt, &wt_path, "hello", &config, PermissionLevel::Normal).unwrap();

        // Normal should NOT inject settings.local.json
        let settings_path = wt_path.join(".claude/settings.local.json");
        assert!(
            !settings_path.exists(),
            "settings.local.json should NOT be created for normal permission"
        );
    }

    #[test]
    fn launch_fails_for_nonexistent_command() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();

        let wt_path = dir.path().join("worktree");
        std::fs::create_dir_all(&wt_path).unwrap();

        let wt = test_worktree("test-wt");
        let config = test_config("nonexistent-command-12345");

        let result = launch_with_prompt(&wt, &wt_path, "hello", &config, PermissionLevel::Normal);
        assert!(result.is_err(), "should fail for nonexistent command");
    }
}
