use serde::{Deserialize, Serialize};

use crate::remote::host::RemoteHost;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub worktree: WorktreeConfig,
    #[serde(default)]
    pub setup: SetupConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub handoff: HandoffConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub container: ContainerConfig,
    /// Registered remote hosts for running worktrees remotely.
    #[serde(default)]
    pub remote: Vec<RemoteHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeConfig {
    #[serde(default = "default_worktree_dir")]
    pub dir: String,
    #[serde(default = "default_max_ephemeral")]
    pub max_ephemeral: usize,
    #[serde(default = "default_true")]
    pub auto_name: bool,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            dir: default_worktree_dir(),
            max_ephemeral: default_max_ephemeral(),
            auto_name: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupConfig {
    #[serde(default)]
    pub script: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl Default for SetupConfig {
    fn default() -> Self {
        Self {
            script: String::new(),
            timeout_secs: default_timeout(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "default_true")]
    pub auto_launch: bool,
    #[serde(default)]
    pub claude_args: Vec<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auto_launch: true,
            claude_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffConfig {
    #[serde(default = "default_handoff_method")]
    pub method: String,
    #[serde(default = "default_true")]
    pub warn_gitignore: bool,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        Self {
            method: default_handoff_method(),
            warn_gitignore: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub show_diff_stat: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            show_diff_stat: true,
        }
    }
}

fn default_worktree_dir() -> String {
    ".claude/worktrees".to_string()
}

fn default_max_ephemeral() -> usize {
    15
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    120
}

fn default_handoff_method() -> String {
    "patch".to_string()
}

fn default_theme() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// Enable container support (auto-detect Containerfile/devcontainer.json).
    #[serde(default)]
    pub enabled: bool,
    /// Preferred container runtime: "podman", "docker", or "auto".
    #[serde(default = "default_container_runtime")]
    pub runtime: String,
    /// Path to Containerfile (relative to repo root). Overrides auto-detection.
    #[serde(default)]
    pub containerfile: String,
    /// Auto-assign ports per worktree.
    #[serde(default = "default_true")]
    pub auto_ports: bool,
    /// Base port for app allocations.
    #[serde(default = "default_app_base_port")]
    pub app_base_port: u16,
    /// Base port for database allocations.
    #[serde(default = "default_db_base_port")]
    pub db_base_port: u16,
    /// Port names to auto-allocate (e.g., ["app", "db"]).
    #[serde(default = "default_port_names")]
    pub port_names: Vec<String>,
    /// Disk usage warning threshold in bytes (default: 1 GiB).
    #[serde(default = "default_disk_warning_bytes")]
    pub disk_warning_bytes: u64,
    /// Track resource usage periodically.
    #[serde(default)]
    pub track_resources: bool,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            runtime: default_container_runtime(),
            containerfile: String::new(),
            auto_ports: true,
            app_base_port: default_app_base_port(),
            db_base_port: default_db_base_port(),
            port_names: default_port_names(),
            disk_warning_bytes: default_disk_warning_bytes(),
            track_resources: false,
        }
    }
}

fn default_container_runtime() -> String {
    "auto".to_string()
}

fn default_app_base_port() -> u16 {
    3000
}

fn default_db_base_port() -> u16 {
    5432
}

fn default_port_names() -> Vec<String> {
    vec!["app".to_string()]
}

fn default_disk_warning_bytes() -> u64 {
    1_073_741_824 // 1 GiB
}
