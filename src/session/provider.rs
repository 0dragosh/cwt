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

    /// Build provider-specific resume arguments for a prior session id.
    pub fn resume_args(self, session_id: &str) -> Vec<String> {
        match self {
            Self::Claude => vec!["--resume".to_string(), session_id.to_string()],
            // Codex supports `codex resume <session-id>`.
            Self::Codex => vec!["resume".to_string(), session_id.to_string()],
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
    fn provider_default_commands() {
        assert_eq!(SessionProvider::Claude.default_command(), "claude");
        assert_eq!(SessionProvider::Codex.default_command(), "codex");
    }
}
