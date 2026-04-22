use anyhow::{Context, Result};

use super::host::RemoteHost;
use crate::config::model::{PermissionLevel, PermissionsConfig};
use crate::session::provider::SessionProvider;

#[derive(Debug, Clone)]
pub struct RemoteCommandConfig<'a> {
    pub provider: SessionProvider,
    pub command: &'a str,
    pub provider_args: &'a [String],
    pub permission: PermissionLevel,
    pub permissions: &'a PermissionsConfig,
}

impl<'a> RemoteCommandConfig<'a> {
    fn command_or_default(&self) -> String {
        self.provider.resolve_command(self.command)
    }

    fn permission_args(&self) -> Vec<String> {
        self.provider
            .effective_permission_args(self.permission, self.permissions)
    }
}

/// Launch a provider session on a remote host via SSH + tmux.
///
/// This creates a tmux session on the remote machine and runs the configured
/// session CLI inside it.
/// The local user can then attach via `ssh -t host tmux attach -t <session>`.
///
/// Returns the remote tmux session name for tracking.
pub fn launch_remote_session(
    host: &RemoteHost,
    repo_name: &str,
    worktree_name: &str,
    cmd_cfg: &RemoteCommandConfig,
) -> Result<String> {
    let repo_path = format!("{}/{}", host.worktree_dir, repo_name);
    let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);
    let tmux_session = format!("cwt-{}", worktree_name);

    // Build the provider command with proper shell quoting
    let mut provider_parts = vec![cmd_cfg.command_or_default()];
    for arg in cmd_cfg.provider_args {
        provider_parts.push(remote_shell_quote(arg));
    }
    for arg in cmd_cfg.permission_args() {
        provider_parts.push(remote_shell_quote(&arg));
    }
    let provider_cmd = provider_parts.join(" ");

    // Create a tmux session on the remote host and run provider CLI in it
    let remote_cmd = format!(
        "tmux new-session -d -s {} -c {} {} 2>/dev/null || tmux send-keys -t {} {} Enter",
        remote_shell_quote(&tmux_session),
        remote_shell_quote(&wt_path),
        remote_shell_quote(&provider_cmd),
        remote_shell_quote(&tmux_session),
        remote_shell_quote(&provider_cmd),
    );

    host.ssh_exec(&remote_cmd).with_context(|| {
        format!(
            "failed to launch remote session for '{}' on '{}'",
            worktree_name, host.name
        )
    })?;

    Ok(tmux_session)
}

/// Resume a provider session on a remote host.
pub fn resume_remote_session(
    host: &RemoteHost,
    repo_name: &str,
    worktree_name: &str,
    session_id: &str,
    cmd_cfg: &RemoteCommandConfig,
) -> Result<String> {
    let repo_path = format!("{}/{}", host.worktree_dir, repo_name);
    let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);
    let tmux_session = format!("cwt-{}", worktree_name);

    let mut provider_parts = vec![cmd_cfg.command_or_default()];
    for arg in cmd_cfg.provider.resume_args(session_id) {
        provider_parts.push(remote_shell_quote(&arg));
    }
    for arg in cmd_cfg.provider_args {
        provider_parts.push(remote_shell_quote(arg));
    }
    for arg in cmd_cfg.permission_args() {
        provider_parts.push(remote_shell_quote(&arg));
    }
    let provider_cmd = provider_parts.join(" ");

    let remote_cmd = format!(
        "tmux new-session -d -s {} -c {} {} 2>/dev/null || tmux send-keys -t {} {} Enter",
        remote_shell_quote(&tmux_session),
        remote_shell_quote(&wt_path),
        remote_shell_quote(&provider_cmd),
        remote_shell_quote(&tmux_session),
        remote_shell_quote(&provider_cmd),
    );

    host.ssh_exec(&remote_cmd)?;
    Ok(tmux_session)
}

/// Focus/attach to a remote session by opening an SSH connection with tmux attach.
/// This opens a local tmux pane that SSHs into the remote and attaches to the session.
pub fn focus_remote_session(host: &RemoteHost, worktree_name: &str) -> Result<String> {
    let tmux_session = format!("cwt-{}", worktree_name);

    // Build SSH command to attach to remote tmux session
    let mut ssh_args = vec!["ssh".to_string()];
    if host.port != 22 {
        ssh_args.push("-p".to_string());
        ssh_args.push(host.port.to_string());
    }
    if !host.identity_file.is_empty() {
        ssh_args.push("-i".to_string());
        ssh_args.push(host.identity_file.clone());
    }
    ssh_args.push("-t".to_string()); // Force TTY allocation
    ssh_args.push(host.ssh_dest());
    ssh_args.push(format!(
        "tmux attach -t {}",
        remote_shell_quote(&tmux_session)
    ));

    let ssh_command = ssh_args.join(" ");
    let pane_title = format!("cwt:remote:{}:{}", host.name, worktree_name);

    // Create a local tmux pane that runs the SSH command
    let pane_id = crate::tmux::pane::create_pane(
        &std::env::current_dir().unwrap_or_default(),
        &ssh_command,
        &pane_title,
    )
    .with_context(|| {
        format!(
            "failed to open SSH pane for '{}' on '{}'",
            worktree_name, host.name
        )
    })?;

    Ok(pane_id)
}

/// Check if a remote tmux session is still running.
pub fn is_remote_session_alive(host: &RemoteHost, worktree_name: &str) -> bool {
    let tmux_session = format!("cwt-{}", worktree_name);
    let cmd = format!(
        "tmux has-session -t {} 2>/dev/null && echo alive",
        remote_shell_quote(&tmux_session)
    );

    host.ssh_exec_fallible(&cmd)
        .map(|(stdout, _, success)| success && stdout.trim() == "alive")
        .unwrap_or(false)
}

/// Kill a remote tmux session.
pub fn kill_remote_session(host: &RemoteHost, worktree_name: &str) -> Result<()> {
    let tmux_session = format!("cwt-{}", worktree_name);
    let cmd = format!(
        "tmux kill-session -t {} 2>/dev/null || true",
        remote_shell_quote(&tmux_session)
    );
    let _ = host.ssh_exec_fallible(&cmd);
    Ok(())
}

/// Get the status of a remote session by checking its tmux pane.
/// Returns a rough status based on whether the session exists and what command is running.
pub fn check_remote_session_status(host: &RemoteHost, worktree_name: &str) -> RemoteSessionStatus {
    let tmux_session = format!("cwt-{}", worktree_name);

    // Check if tmux session exists
    let check_cmd = format!(
        "tmux has-session -t {} 2>/dev/null && tmux display-message -t {} -p '#{{pane_current_command}}' 2>/dev/null",
        remote_shell_quote(&tmux_session),
        remote_shell_quote(&tmux_session)
    );

    match host.ssh_exec_fallible(&check_cmd) {
        Ok((stdout, _, true)) => {
            let command = stdout.trim().to_string();
            if SessionProvider::matches_any_process(&command) {
                RemoteSessionStatus::Running
            } else if command.is_empty() {
                RemoteSessionStatus::Unknown
            } else {
                // Session exists but the provider CLI has exited.
                RemoteSessionStatus::Done
            }
        }
        _ => RemoteSessionStatus::NoSession,
    }
}

/// Status of a remote session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteSessionStatus {
    Running,
    Done,
    NoSession,
    Unknown,
}

/// Open a shell on a remote worktree via SSH in a local tmux pane.
pub fn open_remote_shell(
    host: &RemoteHost,
    repo_name: &str,
    worktree_name: &str,
) -> Result<String> {
    let repo_path = format!("{}/{}", host.worktree_dir, repo_name);
    let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);

    let mut ssh_args = vec!["ssh".to_string()];
    if host.port != 22 {
        ssh_args.push("-p".to_string());
        ssh_args.push(host.port.to_string());
    }
    if !host.identity_file.is_empty() {
        ssh_args.push("-i".to_string());
        ssh_args.push(host.identity_file.clone());
    }
    ssh_args.push("-t".to_string());
    ssh_args.push(host.ssh_dest());
    ssh_args.push(format!(
        "cd {} && exec $SHELL -l",
        remote_shell_quote(&wt_path)
    ));

    let ssh_command = ssh_args.join(" ");
    let pane_title = format!("cwt:shell:{}:{}", host.name, worktree_name);

    let pane_id = crate::tmux::pane::create_pane(
        &std::env::current_dir().unwrap_or_default(),
        &ssh_command,
        &pane_title,
    )?;

    Ok(pane_id)
}

/// Shell-quote a string for safe embedding in remote SSH/tmux commands.
/// Wraps in single quotes and escapes any embedded single quotes.
fn remote_shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{PermissionLevelConfig, PermissionsConfig};

    #[test]
    fn remote_config_resolves_pi_default_command() {
        let permissions = PermissionsConfig::default();
        let cfg = RemoteCommandConfig {
            provider: SessionProvider::Pi,
            command: "claude",
            provider_args: &[],
            permission: crate::config::model::PermissionLevel::Normal,
            permissions: &permissions,
        };

        assert_eq!(cfg.command_or_default(), "pi");
    }

    #[test]
    fn remote_pi_uses_configured_permission_args() {
        let permissions = PermissionsConfig {
            elevated: PermissionLevelConfig {
                extra_args: vec!["--allow-write".to_string()],
                settings_override: None,
            },
            ..PermissionsConfig::default()
        };
        let cfg = RemoteCommandConfig {
            provider: SessionProvider::Pi,
            command: "",
            provider_args: &[],
            permission: crate::config::model::PermissionLevel::Elevated,
            permissions: &permissions,
        };

        assert_eq!(cfg.permission_args(), vec!["--allow-write"]);
    }
}
