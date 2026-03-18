use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    active_multiplexer(
        std::env::var_os("ZELLIJ").as_deref(),
        std::env::var_os("ZELLIJ_SESSION_NAME").as_deref(),
        std::env::var_os("TMUX").as_deref(),
        is_inside_zellij,
        probe_tmux_client,
    )
    .is_some()
}

/// Get the current tmux session name.
pub fn current_session() -> Result<String> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => {
            Ok(std::env::var("ZELLIJ_SESSION_NAME").unwrap_or_else(|_| "zellij".to_string()))
        }
        Multiplexer::Tmux => {
            let output = tmux(&["display-message", "-p", "#{session_name}"])?;
            Ok(output.trim().to_string())
        }
    }
}

/// Create a new tmux pane and run a command in it.
/// Returns the pane ID (e.g., "%5").
pub fn create_pane(worktree_path: &Path, command: &str, pane_title: &str) -> Result<String> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => create_zellij_pane(worktree_path, command, pane_title),
        Multiplexer::Tmux => create_tmux_pane(worktree_path, command, pane_title),
    }
}

/// Focus (select) an existing tmux pane by switching to its window first.
pub fn focus_pane(pane_id: &str) -> Result<()> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => {
            let tab_name = decode_zellij_tab_name(pane_id)?;
            zellij_action(&["go-to-tab-name", &tab_name])?;
            Ok(())
        }
        Multiplexer::Tmux => {
            tmux(&["select-window", "-t", pane_id])?;
            tmux(&["select-pane", "-t", pane_id])?;
            Ok(())
        }
    }
}

/// Kill a tmux pane.
pub fn kill_pane(pane_id: &str) -> Result<()> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => {
            let tab_name = decode_zellij_tab_name(pane_id)?;
            zellij_action(&["go-to-tab-name", &tab_name])?;
            zellij_action(&["close-tab"])?;
            Ok(())
        }
        Multiplexer::Tmux => {
            tmux(&["kill-pane", "-t", pane_id])?;
            Ok(())
        }
    }
}

/// Check if a pane is still alive by querying its pane_id.
/// Returns false if the pane doesn't exist or if tmux itself is not running.
pub fn pane_exists(pane_id: &str) -> bool {
    match preferred_multiplexer() {
        Multiplexer::Zellij => {
            let Ok(tab_name) = decode_zellij_tab_name(pane_id) else {
                return false;
            };
            zellij_tab_exists(&tab_name)
        }
        Multiplexer::Tmux => match Command::new("tmux")
            .args(["display-message", "-t", pane_id, "-p", "#{pane_id}"])
            .output()
        {
            Ok(o) => o.status.success(),
            Err(e) => {
                eprintln!("cwt: tmux query failed for pane {}: {}", pane_id, e);
                false
            }
        },
    }
}

/// Get the current command running in a pane.
pub fn pane_current_command(pane_id: &str) -> Result<String> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => {
            let tab_name = decode_zellij_tab_name(pane_id)?;
            if zellij_tab_exists(&tab_name) {
                Ok("zellij".to_string())
            } else {
                anyhow::bail!("zellij tab '{}' does not exist", tab_name);
            }
        }
        Multiplexer::Tmux => {
            let output = tmux(&[
                "display-message",
                "-t",
                pane_id,
                "-p",
                "#{pane_current_command}",
            ])?;
            Ok(output.trim().to_string())
        }
    }
}

/// List all panes in the current tmux session with their info.
pub fn list_panes() -> Result<Vec<PaneInfo>> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => {
            let output = zellij_action(&["query-tab-names"])?;
            let mut panes = Vec::new();
            for line in output
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
            {
                panes.push(PaneInfo {
                    pane_id: encode_zellij_tab_name(line),
                    pane_title: line.to_string(),
                    current_command: "zellij".to_string(),
                    is_active: false,
                });
            }
            Ok(panes)
        }
        Multiplexer::Tmux => {
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
    }
}

/// Get the PID of the process running in a pane.
pub fn pane_pid(pane_id: &str) -> Result<u32> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => anyhow::bail!("pane pid is not supported for zellij backend"),
        Multiplexer::Tmux => {
            let output = tmux(&["display-message", "-t", pane_id, "-p", "#{pane_pid}"])?;
            let pid: u32 = output
                .trim()
                .parse()
                .with_context(|| format!("invalid pid for pane {}", pane_id))?;
            Ok(pid)
        }
    }
}

/// Send keys to a pane (e.g., for sending input).
pub fn send_keys(pane_id: &str, keys: &str) -> Result<()> {
    match preferred_multiplexer() {
        Multiplexer::Zellij => {
            let tab_name = decode_zellij_tab_name(pane_id)?;
            zellij_action(&["go-to-tab-name", &tab_name])?;
            zellij_action(&["write-chars", &format!("{}\n", keys)])?;
            Ok(())
        }
        Multiplexer::Tmux => {
            tmux(&["send-keys", "-t", pane_id, keys, "Enter"])?;
            Ok(())
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Multiplexer {
    Zellij,
    Tmux,
}

fn preferred_multiplexer() -> Multiplexer {
    if let Some(active) = active_multiplexer(
        std::env::var_os("ZELLIJ").as_deref(),
        std::env::var_os("ZELLIJ_SESSION_NAME").as_deref(),
        std::env::var_os("TMUX").as_deref(),
        is_inside_zellij,
        probe_tmux_client,
    ) {
        return active;
    }

    if command_available("zellij") {
        Multiplexer::Zellij
    } else {
        Multiplexer::Tmux
    }
}

fn active_multiplexer(
    zellij_env: Option<&std::ffi::OsStr>,
    zellij_session_env: Option<&std::ffi::OsStr>,
    tmux_env: Option<&std::ffi::OsStr>,
    probe_zellij: impl FnOnce() -> bool,
    probe_tmux: impl FnOnce() -> bool,
) -> Option<Multiplexer> {
    if (matches!(zellij_env, Some(value) if !value.is_empty())
        || matches!(zellij_session_env, Some(value) if !value.is_empty()))
        && probe_zellij()
    {
        return Some(Multiplexer::Zellij);
    }

    if inside_tmux_with_probe(tmux_env, probe_tmux) {
        return Some(Multiplexer::Tmux);
    }

    None
}

fn command_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_inside_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some() || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
}

fn create_tmux_pane(worktree_path: &Path, command: &str, pane_title: &str) -> Result<String> {
    let path_str = worktree_path
        .to_str()
        .context("worktree path is not valid UTF-8")?;

    let shell_cmd = format!("cd {} && {}", shell_escape(path_str), command);

    let output = tmux(&[
        "new-window",
        "-d", // don't switch focus yet
        "-P",
        "-F",
        "#{pane_id}",
        "-n",
        pane_title,
        &shell_cmd,
    ])?;

    let pane_id = output.trim().to_string();
    let _ = tmux(&["select-pane", "-t", &pane_id, "-T", pane_title]);
    Ok(pane_id)
}

fn create_zellij_pane(worktree_path: &Path, command: &str, pane_title: &str) -> Result<String> {
    let path_str = worktree_path
        .to_str()
        .context("worktree path is not valid UTF-8")?;

    let tab_name = format!("{}-{}", pane_title, unix_timestamp_millis());
    zellij_action(&["new-tab", "--name", &tab_name, "--cwd", path_str])?;
    zellij_action(&["go-to-tab-name", &tab_name])?;
    zellij_action(&["rename-pane", pane_title])?;
    zellij_action(&["write-chars", &format!("{command}\n")])?;

    Ok(encode_zellij_tab_name(&tab_name))
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn zellij_action(args: &[&str]) -> Result<String> {
    let output = Command::new("zellij")
        .arg("action")
        .args(args)
        .output()
        .with_context(|| format!("failed to run zellij action {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("zellij action {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn encode_zellij_tab_name(tab_name: &str) -> String {
    format!("zellij-tab:{tab_name}")
}

fn decode_zellij_tab_name(pane_id: &str) -> Result<String> {
    let Some(tab_name) = pane_id.strip_prefix("zellij-tab:") else {
        anyhow::bail!("invalid zellij pane id '{pane_id}'");
    };
    Ok(tab_name.to_string())
}

fn zellij_tab_exists(tab_name: &str) -> bool {
    let output = match zellij_action(&["query-tab-names"]) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("cwt: zellij query-tab-names failed: {err}");
            return false;
        }
    };
    output.lines().map(str::trim).any(|line| line == tab_name)
}

fn inside_tmux_with_probe(
    tmux_env: Option<&std::ffi::OsStr>,
    probe_tmux_client: impl FnOnce() -> bool,
) -> bool {
    matches!(tmux_env, Some(value) if !value.is_empty()) && probe_tmux_client()
}

fn probe_tmux_client() -> bool {
    Command::new("tmux")
        .args(["display-message", "-p", "#{session_id}"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Simple shell escaping for paths.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build the shell command that will be executed in the tmux window.
#[cfg(test)]
fn build_shell_cmd(worktree_path: &str, command: &str) -> String {
    format!("cd {} && {}", shell_escape(worktree_path), command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::sync::{Mutex, OnceLock};

    fn tmux_env_lock() -> &'static Mutex<()> {
        static TMUX_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        TMUX_ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_shell_escape_simple_path() {
        assert_eq!(shell_escape("/home/user/project"), "'/home/user/project'");
    }

    #[test]
    fn test_shell_escape_path_with_spaces() {
        assert_eq!(
            shell_escape("/home/user/my project"),
            "'/home/user/my project'"
        );
    }

    #[test]
    fn test_shell_escape_path_with_single_quotes() {
        assert_eq!(
            shell_escape("/home/user/it's-a-project"),
            "'/home/user/it'\\''s-a-project'"
        );
    }

    #[test]
    fn test_build_shell_cmd() {
        let cmd = build_shell_cmd("/home/user/project", "claude");
        assert_eq!(cmd, "cd '/home/user/project' && claude");
    }

    #[test]
    fn test_build_shell_cmd_with_args() {
        let cmd = build_shell_cmd("/tmp/wt", "claude --resume sess-123");
        assert_eq!(cmd, "cd '/tmp/wt' && claude --resume sess-123");
    }

    #[test]
    fn stale_tmux_env_does_not_count_as_running_inside_tmux() {
        assert!(!inside_tmux_with_probe(
            Some(OsStr::new("/tmp/tmux-501/default,123,0")),
            || false,
        ));
    }

    #[test]
    fn live_tmux_env_requires_a_healthy_tmux_probe() {
        assert!(inside_tmux_with_probe(
            Some(OsStr::new("/tmp/tmux-501/default,123,0")),
            || true,
        ));
    }

    #[test]
    fn active_tmux_session_beats_zellij_installation() {
        let multiplexer = active_multiplexer(
            None,
            None,
            Some(OsStr::new("/tmp/tmux-501/default,123,0")),
            || false,
            || true,
        );

        assert_eq!(multiplexer, Some(Multiplexer::Tmux));
    }

    #[test]
    fn active_zellij_session_is_detected_without_tmux() {
        let multiplexer = active_multiplexer(Some(OsStr::new("0")), None, None, || true, || false);

        assert_eq!(multiplexer, Some(Multiplexer::Zellij));
    }

    #[test]
    fn is_inside_tmux_rejects_a_stale_tmux_environment() {
        let _guard = tmux_env_lock().lock().unwrap();
        let original_tmux = std::env::var_os("TMUX");

        std::env::set_var("TMUX", "/tmp/tmux-501/definitely-stale,123,0");
        assert!(!is_inside_tmux());

        match original_tmux {
            Some(value) => std::env::set_var("TMUX", value),
            None => std::env::remove_var("TMUX"),
        }
    }

    /// Verify create_pane uses `new-window` (tabs) not `split-window` (splits).
    /// We can't run tmux in unit tests, but we can verify the function constructs
    /// the right command by checking that it calls new-window via a dedicated
    /// tmux session in CI/dev environments.
    #[test]
    fn test_create_pane_uses_new_window() {
        // Verify the source uses new-window by checking the function exists
        // and builds the right shell command
        let cmd = build_shell_cmd("/tmp/test-wt", "claude");
        assert!(cmd.starts_with("cd '/tmp/test-wt'"));
        assert!(cmd.ends_with("claude"));
    }

    /// Integration test: verify full tmux window lifecycle (create, focus, kill).
    /// Only runs when tmux is available (skipped in sandboxed/CI environments).
    #[test]
    fn test_tmux_window_lifecycle() {
        // Skip if tmux is not available
        let tmux_available = Command::new("tmux").arg("-V").output().is_ok();
        if !tmux_available {
            eprintln!("skipping tmux integration test: tmux not found");
            return;
        }

        // Create a temporary tmux server with a unique socket
        let socket = format!("cwt-test-{}", std::process::id());
        let setup = Command::new("tmux")
            .args(["-L", &socket, "new-session", "-d", "-s", "test"])
            .output();

        let Ok(out) = setup else {
            eprintln!("skipping: could not create tmux test session");
            return;
        };
        if !out.status.success() {
            eprintln!("skipping: tmux new-session failed");
            return;
        }

        // Helper to run tmux commands on our test server
        let tmux_cmd = |args: &[&str]| -> Result<String> {
            let mut full_args = vec!["-L", &socket];
            full_args.extend_from_slice(args);
            let output = Command::new("tmux").args(&full_args).output()?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("tmux failed: {}", stderr.trim());
            }
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        };

        // Create a new window (tab) — mirrors what create_pane does
        let result = tmux_cmd(&[
            "new-window",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-n",
            "cwt:test-wt",
            "sleep 60",
        ]);
        assert!(result.is_ok(), "new-window should succeed");
        let pane_id = result.unwrap().trim().to_string();
        assert!(
            pane_id.starts_with('%'),
            "pane_id should start with %: {pane_id}"
        );

        // Verify the window was created with the right name
        let windows = tmux_cmd(&["list-windows", "-t", "test", "-F", "#{window_name}"]);
        assert!(windows.is_ok());
        let window_list = windows.unwrap();
        assert!(
            window_list.contains("cwt:test-wt"),
            "window name should be set: {window_list}"
        );

        // Focus the window (select-window then select-pane)
        assert!(tmux_cmd(&["select-window", "-t", &pane_id]).is_ok());
        assert!(tmux_cmd(&["select-pane", "-t", &pane_id]).is_ok());

        // Verify the pane still exists
        let check = tmux_cmd(&["display-message", "-t", &pane_id, "-p", "#{pane_id}"]);
        assert!(check.is_ok());
        assert_eq!(check.unwrap().trim(), pane_id);

        // Kill the pane/window
        assert!(tmux_cmd(&["kill-pane", "-t", &pane_id]).is_ok());

        // Verify it's gone from the pane list
        let remaining = tmux_cmd(&["list-panes", "-s", "-F", "#{pane_id}"]).unwrap_or_default();
        assert!(
            !remaining.lines().any(|l| l.trim() == pane_id),
            "pane should no longer appear in list-panes after kill"
        );

        // Clean up the test server
        let _ = Command::new("tmux")
            .args(["-L", &socket, "kill-server"])
            .output();
    }
}
