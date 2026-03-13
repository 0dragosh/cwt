use serde::{Deserialize, Serialize};

use crate::remote::host::RemoteHost;

/// Permission level for Claude Code sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    /// Plain `claude` — asks for permission on each tool use.
    #[default]
    Normal,
    /// Injects sandbox settings into `.claude/settings.local.json` before launch.
    Elevated,
    /// Appends `--dangerously-skip-permissions` — full autonomy, no sandbox.
    ElevatedUnsandboxed,
}

impl PermissionLevel {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Elevated => "Elevated",
            Self::ElevatedUnsandboxed => "Unsandboxed",
        }
    }

    /// Short label for badges.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Normal => "N",
            Self::Elevated => "E",
            Self::ElevatedUnsandboxed => "U!",
        }
    }

    /// Cycle to the next permission level.
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Normal => Self::Elevated,
            Self::Elevated => Self::ElevatedUnsandboxed,
            Self::ElevatedUnsandboxed => Self::Normal,
        }
    }
}

/// Configuration for a single permission level.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionLevelConfig {
    /// Extra CLI arguments appended to the claude command.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// JSON value merged into `<worktree>/.claude/settings.local.json` before launch.
    #[serde(default)]
    pub settings_override: Option<serde_json::Value>,
}

/// Per-level permission configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    #[serde(default)]
    pub normal: PermissionLevelConfig,
    #[serde(default = "default_elevated_config")]
    pub elevated: PermissionLevelConfig,
    #[serde(default = "default_elevated_unsandboxed_config")]
    pub elevated_unsandboxed: PermissionLevelConfig,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            normal: PermissionLevelConfig::default(),
            elevated: default_elevated_config(),
            elevated_unsandboxed: default_elevated_unsandboxed_config(),
        }
    }
}

impl PermissionsConfig {
    /// Get the config for a given permission level.
    pub fn get(&self, level: PermissionLevel) -> &PermissionLevelConfig {
        match level {
            PermissionLevel::Normal => &self.normal,
            PermissionLevel::Elevated => &self.elevated,
            PermissionLevel::ElevatedUnsandboxed => &self.elevated_unsandboxed,
        }
    }
}

fn default_elevated_config() -> PermissionLevelConfig {
    PermissionLevelConfig {
        extra_args: Vec::new(),
        settings_override: Some(serde_json::json!({
            "permissions": {
                "allow": [],
                "deny": []
            },
            "sandbox": {
                "enabled": true,
                "autoAllowBashIfSandboxed": true,
                "allowUnsandboxedCommands": false
            }
        })),
    }
}

fn default_elevated_unsandboxed_config() -> PermissionLevelConfig {
    PermissionLevelConfig {
        extra_args: vec!["--dangerously-skip-permissions".to_string()],
        settings_override: None,
    }
}

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
    /// The command to launch (default: "claude"). Allows using custom wrappers.
    #[serde(default = "default_session_command")]
    pub command: String,
    #[serde(default)]
    pub claude_args: Vec<String>,
    /// Default permission level for new sessions.
    #[serde(default)]
    pub default_permission: PermissionLevel,
    /// Per-level permission configurations.
    #[serde(default)]
    pub permissions: PermissionsConfig,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auto_launch: true,
            command: default_session_command(),
            claude_args: Vec::new(),
            default_permission: PermissionLevel::default(),
            permissions: PermissionsConfig::default(),
        }
    }
}

fn default_session_command() -> String {
    "claude".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- PermissionLevel ---

    #[test]
    fn permission_level_default_is_normal() {
        assert_eq!(PermissionLevel::default(), PermissionLevel::Normal);
    }

    #[test]
    fn cycle_next_wraps_around() {
        let n = PermissionLevel::Normal;
        let e = n.cycle_next();
        let u = e.cycle_next();
        let back = u.cycle_next();
        assert_eq!(e, PermissionLevel::Elevated);
        assert_eq!(u, PermissionLevel::ElevatedUnsandboxed);
        assert_eq!(back, PermissionLevel::Normal);
    }

    #[test]
    fn labels_are_distinct() {
        let levels = [
            PermissionLevel::Normal,
            PermissionLevel::Elevated,
            PermissionLevel::ElevatedUnsandboxed,
        ];
        let labels: Vec<_> = levels.iter().map(|l| l.label()).collect();
        let short: Vec<_> = levels.iter().map(|l| l.short_label()).collect();
        // No duplicates
        assert_eq!(labels.len(), 3);
        assert_ne!(labels[0], labels[1]);
        assert_ne!(labels[1], labels[2]);
        assert_ne!(short[0], short[1]);
        assert_ne!(short[1], short[2]);
    }

    #[test]
    fn permission_level_serde_round_trip() {
        for level in [
            PermissionLevel::Normal,
            PermissionLevel::Elevated,
            PermissionLevel::ElevatedUnsandboxed,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: PermissionLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, back);
        }
    }

    #[test]
    fn permission_level_serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&PermissionLevel::ElevatedUnsandboxed).unwrap(),
            "\"elevated_unsandboxed\""
        );
        assert_eq!(
            serde_json::to_string(&PermissionLevel::Normal).unwrap(),
            "\"normal\""
        );
    }

    // --- PermissionsConfig ---

    #[test]
    fn permissions_get_dispatches_correctly() {
        let cfg = PermissionsConfig::default();
        // Normal has no extra_args and no settings_override
        let normal = cfg.get(PermissionLevel::Normal);
        assert!(normal.extra_args.is_empty());
        assert!(normal.settings_override.is_none());

        // Elevated has settings_override with sandbox
        let elevated = cfg.get(PermissionLevel::Elevated);
        assert!(elevated.extra_args.is_empty());
        let settings = elevated.settings_override.as_ref().unwrap();
        assert_eq!(settings["sandbox"]["enabled"], true);
        assert_eq!(settings["sandbox"]["autoAllowBashIfSandboxed"], true);

        // Unsandboxed has --dangerously-skip-permissions
        let unsandboxed = cfg.get(PermissionLevel::ElevatedUnsandboxed);
        assert_eq!(
            unsandboxed.extra_args,
            vec!["--dangerously-skip-permissions"]
        );
        assert!(unsandboxed.settings_override.is_none());
    }

    #[test]
    fn permissions_config_custom_overrides_defaults() {
        let mut cfg = PermissionsConfig::default();
        cfg.elevated.extra_args = vec!["--verbose".to_string()];
        cfg.elevated_unsandboxed.extra_args = vec!["--custom-flag".to_string()];

        assert_eq!(
            cfg.get(PermissionLevel::Elevated).extra_args,
            vec!["--verbose"]
        );
        assert_eq!(
            cfg.get(PermissionLevel::ElevatedUnsandboxed).extra_args,
            vec!["--custom-flag"]
        );
    }

    // --- SessionConfig with permissions ---

    #[test]
    fn session_config_toml_round_trip_with_permissions() {
        let toml_str = r#"
auto_launch = false
command = "my-claude"
default_permission = "elevated"

[permissions.normal]
extra_args = []

[permissions.elevated]
extra_args = ["--verbose"]

[permissions.elevated_unsandboxed]
extra_args = ["--dangerously-skip-permissions", "--fast"]
"#;
        let cfg: SessionConfig = toml::from_str(toml_str).unwrap();
        assert!(!cfg.auto_launch);
        assert_eq!(cfg.command, "my-claude");
        assert_eq!(cfg.default_permission, PermissionLevel::Elevated);
        assert_eq!(cfg.permissions.elevated.extra_args, vec!["--verbose"]);
        assert_eq!(
            cfg.permissions.elevated_unsandboxed.extra_args,
            vec!["--dangerously-skip-permissions", "--fast"]
        );
    }

    #[test]
    fn session_config_missing_permission_fields_uses_defaults() {
        // A minimal TOML that omits all permission fields
        let toml_str = r#"
auto_launch = true
"#;
        let cfg: SessionConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.default_permission, PermissionLevel::Normal);
        // Elevated defaults should still have sandbox settings
        assert!(cfg.permissions.elevated.settings_override.is_some());
        assert_eq!(
            cfg.permissions.elevated_unsandboxed.extra_args,
            vec!["--dangerously-skip-permissions"]
        );
    }

    #[test]
    fn session_config_partial_permissions_override() {
        // Override only elevated, leave others as defaults
        let toml_str = r#"
[permissions.elevated]
extra_args = ["--custom"]
"#;
        let cfg: SessionConfig = toml::from_str(toml_str).unwrap();
        // Elevated is overridden — no settings_override since TOML didn't specify it
        assert_eq!(cfg.permissions.elevated.extra_args, vec!["--custom"]);
        assert!(cfg.permissions.elevated.settings_override.is_none());
        // Unsandboxed keeps its default
        assert_eq!(
            cfg.permissions.elevated_unsandboxed.extra_args,
            vec!["--dangerously-skip-permissions"]
        );
        // Normal keeps its default
        assert!(cfg.permissions.normal.extra_args.is_empty());
    }

    // --- Full Config round-trip ---

    #[test]
    fn full_config_toml_preserves_permission_level() {
        let mut config = Config::default();
        config.session.default_permission = PermissionLevel::ElevatedUnsandboxed;

        let serialized = toml::to_string_pretty(&config).unwrap();
        let loaded: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(
            loaded.session.default_permission,
            PermissionLevel::ElevatedUnsandboxed
        );
    }
}
