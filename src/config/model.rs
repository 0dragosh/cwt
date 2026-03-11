use serde::{Deserialize, Serialize};

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
