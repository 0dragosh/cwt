use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Events that cwt receives from Claude Code hooks via the Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HookEvent {
    /// A worktree was created (possibly externally by Claude Code).
    WorktreeCreated {
        worktree: String,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
    },
    /// A worktree was removed externally.
    WorktreeRemoved {
        worktree: String,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
    },
    /// A Claude Code session stopped.
    SessionStopped {
        worktree: String,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
        #[serde(default)]
        data: Option<SessionStopData>,
    },
    /// A Claude Code session sent a notification (waiting for input).
    SessionNotification {
        worktree: String,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
        #[serde(default)]
        message: Option<String>,
    },
    /// A subagent stopped within a Claude Code session.
    SubagentStopped {
        worktree: String,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<DateTime<Utc>>,
    },
}

/// Additional data for SessionStopped events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStopData {
    #[serde(default)]
    pub exit_reason: Option<String>,
}

impl HookEvent {
    /// Get the worktree name associated with this event.
    pub fn worktree_name(&self) -> &str {
        match self {
            HookEvent::WorktreeCreated { worktree, .. } => worktree,
            HookEvent::WorktreeRemoved { worktree, .. } => worktree,
            HookEvent::SessionStopped { worktree, .. } => worktree,
            HookEvent::SessionNotification { worktree, .. } => worktree,
            HookEvent::SubagentStopped { worktree, .. } => worktree,
        }
    }

    /// Parse a JSON string into a HookEvent.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_stopped() {
        let json = r#"{
            "event": "session_stopped",
            "worktree": "feature-auth",
            "session_id": "abc123",
            "timestamp": "2026-03-11T15:30:00Z",
            "data": {
                "exit_reason": "complete"
            }
        }"#;
        let event = HookEvent::from_json(json).unwrap();
        assert_eq!(event.worktree_name(), "feature-auth");
        if let HookEvent::SessionStopped { session_id, data, .. } = &event {
            assert_eq!(session_id.as_deref(), Some("abc123"));
            assert_eq!(
                data.as_ref().and_then(|d| d.exit_reason.as_deref()),
                Some("complete")
            );
        } else {
            panic!("expected SessionStopped");
        }
    }

    #[test]
    fn test_parse_worktree_created() {
        let json = r#"{
            "event": "worktree_created",
            "worktree": "bold-oak-a3f2"
        }"#;
        let event = HookEvent::from_json(json).unwrap();
        assert_eq!(event.worktree_name(), "bold-oak-a3f2");
    }

    #[test]
    fn test_parse_session_notification() {
        let json = r#"{
            "event": "session_notification",
            "worktree": "bugfix",
            "message": "Waiting for user input"
        }"#;
        let event = HookEvent::from_json(json).unwrap();
        if let HookEvent::SessionNotification { message, .. } = &event {
            assert_eq!(message.as_deref(), Some("Waiting for user input"));
        } else {
            panic!("expected SessionNotification");
        }
    }
}
