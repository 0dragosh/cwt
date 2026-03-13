use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::{Duration, Instant};

/// Configuration for a remote host where worktrees can be created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHost {
    /// Friendly name for the host (e.g., "fenrir")
    pub name: String,
    /// SSH hostname or IP address
    pub host: String,
    /// SSH user
    #[serde(default)]
    pub user: String,
    /// Directory on the remote host where worktrees are stored
    #[serde(default = "default_worktree_dir")]
    pub worktree_dir: String,
    /// Optional SSH port (default: 22)
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// Optional SSH identity file
    #[serde(default)]
    pub identity_file: String,
}

fn default_worktree_dir() -> String {
    "/data/worktrees".to_string()
}

fn default_ssh_port() -> u16 {
    22
}

impl RemoteHost {
    /// Build the SSH destination string (user@host).
    pub fn ssh_dest(&self) -> String {
        if self.user.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.user, self.host)
        }
    }

    /// Build base SSH args (port, identity file).
    pub fn ssh_base_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.port != 22 {
            args.push("-p".to_string());
            args.push(self.port.to_string());
        }
        if !self.identity_file.is_empty() {
            args.push("-i".to_string());
            args.push(self.identity_file.clone());
        }
        // Batch mode options for non-interactive use
        args.push("-o".to_string());
        args.push("BatchMode=yes".to_string());
        args.push("-o".to_string());
        args.push("ConnectTimeout=10".to_string());
        args
    }

    /// Run a command on the remote host via SSH and return stdout.
    pub fn ssh_exec(&self, command: &str) -> Result<String> {
        let mut cmd = Command::new("ssh");
        for arg in self.ssh_base_args() {
            cmd.arg(arg);
        }
        cmd.arg(self.ssh_dest());
        cmd.arg(command);

        let output = cmd
            .output()
            .with_context(|| format!("failed to SSH to {}", self.name))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("SSH command failed on '{}': {}", self.name, stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run a command on the remote host via SSH, returning (stdout, stderr, success).
    pub fn ssh_exec_fallible(&self, command: &str) -> Result<(String, String, bool)> {
        let mut cmd = Command::new("ssh");
        for arg in self.ssh_base_args() {
            cmd.arg(arg);
        }
        cmd.arg(self.ssh_dest());
        cmd.arg(command);

        let output = cmd
            .output()
            .with_context(|| format!("failed to SSH to {}", self.name))?;

        Ok((
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
            output.status.success(),
        ))
    }

    /// Check if the remote host is reachable via SSH.
    pub fn is_reachable(&self) -> bool {
        let mut cmd = Command::new("ssh");
        for arg in self.ssh_base_args() {
            cmd.arg(arg);
        }
        cmd.arg(self.ssh_dest());
        cmd.arg("echo ok");

        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    }

    /// Measure round-trip latency to the remote host.
    pub fn measure_latency(&self) -> Option<Duration> {
        let start = Instant::now();
        let mut cmd = Command::new("ssh");
        for arg in self.ssh_base_args() {
            cmd.arg(arg);
        }
        cmd.arg(self.ssh_dest());
        cmd.arg("echo ok");

        match cmd.output() {
            Ok(o) if o.status.success() => Some(start.elapsed()),
            _ => None,
        }
    }

    /// Check if git is available on the remote host.
    pub fn has_git(&self) -> bool {
        self.ssh_exec("which git")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    /// Check if tmux is available on the remote host.
    pub fn has_tmux(&self) -> bool {
        self.ssh_exec("which tmux")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    /// Check if claude is available on the remote host.
    pub fn has_claude(&self) -> bool {
        self.ssh_exec("which claude")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    /// Ensure the worktree directory exists on the remote host.
    pub fn ensure_worktree_dir(&self) -> Result<()> {
        self.ssh_exec(&format!("mkdir -p {}", ssh_shell_quote(&self.worktree_dir)))?;
        Ok(())
    }

    /// Run a git command in the remote worktree directory.
    pub fn git_remote(&self, repo_name: &str, args: &[&str]) -> Result<String> {
        let repo_path = format!("{}/{}", self.worktree_dir, repo_name);
        let git_args = args.join(" ");
        let command = format!("cd {} && git {}", ssh_shell_quote(&repo_path), git_args);
        self.ssh_exec(&command)
    }

    /// Clone a repository on the remote host if it doesn't exist.
    pub fn ensure_repo(&self, repo_url: &str, repo_name: &str) -> Result<()> {
        let repo_path = format!("{}/{}", self.worktree_dir, repo_name);

        // Check if repo already exists
        let (_, _, exists) = self.ssh_exec_fallible(&format!("test -d {}/.git", ssh_shell_quote(&repo_path)))?;

        if !exists {
            self.ensure_worktree_dir()?;
            self.ssh_exec(&format!(
                "cd {} && git clone {} {}",
                ssh_shell_quote(&self.worktree_dir),
                ssh_shell_quote(repo_url),
                ssh_shell_quote(repo_name)
            ))
            .with_context(|| {
                format!(
                    "failed to clone {} on remote host '{}'",
                    repo_url, self.name
                )
            })?;
        } else {
            // Fetch latest
            let _ = self.git_remote(repo_name, &["fetch", "--all"]);
        }

        Ok(())
    }

    /// Create a git worktree on the remote host.
    pub fn create_worktree(
        &self,
        repo_name: &str,
        worktree_name: &str,
        branch_name: &str,
        base_branch: &str,
    ) -> Result<String> {
        let repo_path = format!("{}/{}", self.worktree_dir, repo_name);
        let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);

        // Create the worktree
        let cmd = format!(
            "cd {} && git worktree add {} -b {} {}",
            ssh_shell_quote(&repo_path),
            ssh_shell_quote(&wt_path),
            ssh_shell_quote(branch_name),
            ssh_shell_quote(base_branch)
        );
        self.ssh_exec(&cmd).with_context(|| {
            format!(
                "failed to create worktree '{}' on '{}'",
                worktree_name, self.name
            )
        })?;

        Ok(wt_path)
    }

    /// Remove a git worktree on the remote host.
    pub fn remove_worktree(&self, repo_name: &str, worktree_name: &str) -> Result<()> {
        let repo_path = format!("{}/{}", self.worktree_dir, repo_name);
        let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);

        let cmd = format!(
            "cd {} && git worktree remove --force {}",
            ssh_shell_quote(&repo_path),
            ssh_shell_quote(&wt_path)
        );
        self.ssh_exec(&cmd).with_context(|| {
            format!(
                "failed to remove remote worktree '{}' on '{}'",
                worktree_name, self.name
            )
        })?;

        Ok(())
    }

    /// Get the HEAD commit hash of the remote repo.
    pub fn head_commit(&self, repo_name: &str) -> Result<String> {
        let output = self.git_remote(repo_name, &["rev-parse", "HEAD"])?;
        Ok(output.trim().to_string())
    }

    /// Get the diff stat of a remote worktree.
    pub fn diff_stat(&self, repo_name: &str, worktree_name: &str) -> Result<String> {
        let repo_path = format!("{}/{}", self.worktree_dir, repo_name);
        let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);
        let cmd = format!("cd {} && git diff --stat", ssh_shell_quote(&wt_path));
        self.ssh_exec(&cmd)
    }

    /// Get the full diff of a remote worktree.
    pub fn diff_full(&self, repo_name: &str, worktree_name: &str) -> Result<String> {
        let repo_path = format!("{}/{}", self.worktree_dir, repo_name);
        let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);
        let cmd = format!("cd {} && git diff", ssh_shell_quote(&wt_path));
        self.ssh_exec(&cmd)
    }

    /// Check if a remote worktree has uncommitted changes.
    pub fn is_dirty(&self, repo_name: &str, worktree_name: &str) -> Result<bool> {
        let repo_path = format!("{}/{}", self.worktree_dir, repo_name);
        let wt_path = format!("{}/worktrees/{}", repo_path, worktree_name);
        let cmd = format!("cd {} && git status --porcelain", ssh_shell_quote(&wt_path));
        let (stdout, _, _) = self.ssh_exec_fallible(&cmd)?;
        Ok(!stdout.trim().is_empty())
    }
}

/// Shell-quote a string for safe embedding in SSH commands.
/// Wraps in single quotes and escapes any embedded single quotes.
fn ssh_shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Network status for a remote host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkStatus {
    /// Host is reachable with known latency
    Connected(Duration),
    /// Host is unreachable or connection failed
    Disconnected,
    /// Status has not been checked yet
    Unknown,
}

impl NetworkStatus {
    /// Format as a short string for display.
    pub fn label(&self) -> String {
        match self {
            NetworkStatus::Connected(d) => format!("connected ({}ms)", d.as_millis()),
            NetworkStatus::Disconnected => "disconnected".to_string(),
            NetworkStatus::Unknown => "unknown".to_string(),
        }
    }

    /// Return an icon for display in the TUI.
    pub fn icon(&self) -> &'static str {
        match self {
            NetworkStatus::Connected(_) => "[ok]",
            NetworkStatus::Disconnected => "[!!]",
            NetworkStatus::Unknown => "[??]",
        }
    }
}

/// Cached status for a remote host (updated periodically, not on every tick).
#[derive(Debug, Clone)]
pub struct RemoteHostStatus {
    pub name: String,
    pub network: NetworkStatus,
    pub has_git: bool,
    pub has_tmux: bool,
    pub has_claude: bool,
}

impl RemoteHostStatus {
    /// Check a remote host and build its status.
    pub fn check(host: &RemoteHost) -> Self {
        let network = match host.measure_latency() {
            Some(d) => NetworkStatus::Connected(d),
            None => NetworkStatus::Disconnected,
        };

        let (has_git, has_tmux, has_claude) = if network != NetworkStatus::Disconnected {
            (host.has_git(), host.has_tmux(), host.has_claude())
        } else {
            (false, false, false)
        };

        Self {
            name: host.name.clone(),
            network,
            has_git,
            has_tmux,
            has_claude,
        }
    }

    /// Build an unknown/unchecked status.
    pub fn unknown(name: &str) -> Self {
        Self {
            name: name.to_string(),
            network: NetworkStatus::Unknown,
            has_git: false,
            has_tmux: false,
            has_claude: false,
        }
    }
}
