use serde::{Deserialize, Serialize};

/// Supported CLI providers for interactive coding sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionProvider {
    /// Anthropic Claude Code CLI.
    #[default]
    Claude,
    /// OpenAI Codex CLI.
    Codex,
}

impl SessionProvider {
    /// Default executable name for this provider.
    pub fn default_command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Resolve the executable name for this provider.
    ///
    /// Empty commands and stale built-in provider commands should follow the
    /// active provider selection rather than pinning a previous default.
    pub fn resolve_command(self, configured_command: &str) -> String {
        let trimmed = configured_command.trim();
        if trimmed.is_empty()
            || trimmed == Self::Claude.default_command()
            || trimmed == Self::Codex.default_command()
        {
            self.default_command().to_string()
        } else {
            configured_command.to_string()
        }
    }

    /// Human-readable provider name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// Short uppercase provider label for badges.
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Claude => "CL",
            Self::Codex => "CX",
        }
    }

    /// Cycle to the next supported provider.
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Claude => Self::Codex,
            Self::Codex => Self::Claude,
        }
    }

    /// Build provider-specific resume arguments for a prior session id.
    pub fn resume_args(self, session_id: &str) -> Vec<String> {
        match self {
            Self::Claude => vec!["--resume".to_string(), session_id.to_string()],
            // Codex supports `codex resume <session-id>`.
            Self::Codex => vec!["resume".to_string(), session_id.to_string()],
        }
    }

    /// Provider-specific permission flags for the selected mode.
    pub fn permission_args(
        self,
        level: crate::config::model::PermissionLevel,
    ) -> &'static [&'static str] {
        match (self, level) {
            (Self::Codex, crate::config::model::PermissionLevel::Normal) => &[],
            (Self::Codex, crate::config::model::PermissionLevel::Elevated) => &["--full-auto"],
            (Self::Codex, crate::config::model::PermissionLevel::ElevatedUnsandboxed) => {
                &["--dangerously-bypass-approvals-and-sandbox"]
            }
            _ => &[],
        }
    }

    /// Human-readable mode label for status messages.
    pub fn mode_label(self, level: crate::config::model::PermissionLevel) -> &'static str {
        match (self, level) {
            (Self::Codex, crate::config::model::PermissionLevel::Elevated) => "Unsandboxed",
            (Self::Codex, crate::config::model::PermissionLevel::ElevatedUnsandboxed) => {
                "Elevated Unsandboxed"
            }
            (_, crate::config::model::PermissionLevel::Normal) => "Normal",
            (_, crate::config::model::PermissionLevel::Elevated) => "Elevated",
            (_, crate::config::model::PermissionLevel::ElevatedUnsandboxed) => "Unsandboxed",
        }
    }

    /// Return true if a foreground process name likely belongs to this provider.
    pub fn matches_process(self, process_name: &str) -> bool {
        let process_name = process_name.to_ascii_lowercase();
        match self {
            Self::Claude => process_name.contains("claude") || process_name == "node",
            Self::Codex => process_name.contains("codex") || process_name == "node",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_defaults_to_claude() {
        assert_eq!(SessionProvider::default(), SessionProvider::Claude);
    }

    #[test]
    fn provider_serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&SessionProvider::Claude).unwrap(),
            "\"claude\""
        );
        assert_eq!(
            serde_json::to_string(&SessionProvider::Codex).unwrap(),
            "\"codex\""
        );
    }

    #[test]
    fn provider_cycle_next_wraps() {
        assert_eq!(SessionProvider::Claude.cycle_next(), SessionProvider::Codex);
        assert_eq!(SessionProvider::Codex.cycle_next(), SessionProvider::Claude);
    }

    #[test]
    fn codex_permission_args_match_expected_flags() {
        use crate::config::model::PermissionLevel;
        assert_eq!(
            SessionProvider::Codex.permission_args(PermissionLevel::Elevated),
            ["--full-auto"]
        );
        assert_eq!(
            SessionProvider::Codex.permission_args(PermissionLevel::ElevatedUnsandboxed),
            ["--dangerously-bypass-approvals-and-sandbox"]
        );
    }

    #[test]
    fn provider_default_commands() {
        assert_eq!(SessionProvider::Claude.default_command(), "claude");
        assert_eq!(SessionProvider::Codex.default_command(), "codex");
    }

    #[test]
    fn provider_resolve_command_uses_active_provider_for_builtin_defaults() {
        assert_eq!(
            SessionProvider::Codex.resolve_command(""),
            SessionProvider::Codex.default_command()
        );
        assert_eq!(
            SessionProvider::Codex.resolve_command("claude"),
            SessionProvider::Codex.default_command()
        );
        assert_eq!(
            SessionProvider::Claude.resolve_command("codex"),
            SessionProvider::Claude.default_command()
        );
    }

    #[test]
    fn provider_resolve_command_preserves_custom_override() {
        assert_eq!(
            SessionProvider::Codex.resolve_command("/usr/local/bin/custom-codex"),
            "/usr/local/bin/custom-codex"
        );
    }
}
