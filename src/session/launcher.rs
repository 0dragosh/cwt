use anyhow::{Context, Result};
use std::path::Path;

use crate::config::model::{PermissionLevel, PermissionsConfig, SessionConfig};
use crate::env::container::{ContainerRuntime, ContainerStatus};
use crate::tmux;
use crate::worktree::model::Worktree;

/// Launch a new provider session in a tmux pane for the given worktree.
/// If the worktree has a running container, the session runs inside it.
/// Returns the tmux pane ID.
pub fn launch_session(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    config: &SessionConfig,
    permission: PermissionLevel,
    permissions: &PermissionsConfig,
) -> Result<String> {
    if !tmux::pane::is_inside_tmux() {
        anyhow::bail!(
            "cwt sessions require an active terminal multiplexer (zellij preferred, tmux fallback)"
        );
    }

    if config.provider == crate::session::provider::SessionProvider::Claude {
        if let Some(ref settings) = permissions.get(permission).settings_override {
            inject_settings_override(worktree_abs_path, settings)?;
        }
    }

    let command = build_provider_command(worktree, config, None, permission, permissions);
    let pane_title = format!("cwt:{}", worktree.name);

    let pane_id = tmux::pane::create_pane(worktree_abs_path, &command, &pane_title)
        .with_context(|| format!("failed to launch session for '{}'", worktree.name))?;

    Ok(pane_id)
}

/// Resume a previous provider session in a new tmux pane.
/// Uses provider-specific resume arguments to continue the conversation.
/// Returns the tmux pane ID.
pub fn resume_session(
    worktree: &Worktree,
    worktree_abs_path: &Path,
    session_id: &str,
    config: &SessionConfig,
    permission: PermissionLevel,
    permissions: &PermissionsConfig,
) -> Result<String> {
    if !tmux::pane::is_inside_tmux() {
        anyhow::bail!(
            "cwt sessions require an active terminal multiplexer (zellij preferred, tmux fallback)"
        );
    }

    if config.provider == crate::session::provider::SessionProvider::Claude {
        if let Some(ref settings) = permissions.get(permission).settings_override {
            inject_settings_override(worktree_abs_path, settings)?;
        }
    }

    let command =
        build_provider_command(worktree, config, Some(session_id), permission, permissions);
    let pane_title = format!("cwt:{}", worktree.name);

    let pane_id = tmux::pane::create_pane(worktree_abs_path, &command, &pane_title)
        .with_context(|| format!("failed to resume session for '{}'", worktree.name))?;

    Ok(pane_id)
}

/// Build the provider command string, optionally wrapping it in a container exec.
fn build_provider_command(
    worktree: &Worktree,
    config: &SessionConfig,
    resume_session_id: Option<&str>,
    permission: PermissionLevel,
    permissions: &PermissionsConfig,
) -> String {
    let provider = config.provider;
    let command = provider.resolve_command(&config.command);

    let mut cmd_parts = vec![command];

    if let Some(sid) = resume_session_id {
        for arg in provider.resume_args(sid) {
            cmd_parts.push(shell_quote(&arg));
        }
    }

    for arg in &config.provider_args {
        cmd_parts.push(shell_quote(arg));
    }

    let permission_args: Vec<String> =
        if provider == crate::session::provider::SessionProvider::Codex {
            provider
                .permission_args(permission)
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        } else {
            permissions.get(permission).extra_args.clone()
        };

    for arg in permission_args {
        cmd_parts.push(arg);
    }

    let provider_cmd = cmd_parts.join(" ");

    // If the worktree has a running container, exec into it
    if let Some(ref container) = worktree.container {
        if container.status == ContainerStatus::Running {
            if let Some(ref cid) = container.container_id {
                return build_container_exec_command(&container.runtime, cid, &provider_cmd);
            }
            if let Some(ref name) = container.container_name {
                return build_container_exec_command(&container.runtime, name, &provider_cmd);
            }
        }
    }

    provider_cmd
}

/// Build a container exec command that runs the provider CLI inside the container.
fn build_container_exec_command(
    runtime: &ContainerRuntime,
    container_id: &str,
    inner_command: &str,
) -> String {
    format!(
        "{} exec -it -w /workspace {} sh -c '{}'",
        runtime.cmd(),
        shell_quote(container_id),
        inner_command.replace('\'', "'\\''"),
    )
}

/// Shell-quote a string for safe embedding in a command.
/// Wraps in single quotes and escapes any embedded single quotes.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Deep-merge `override_val` into `base`. For objects, recurse; for other types, override.
fn json_deep_merge(base: &mut serde_json::Value, override_val: &serde_json::Value) {
    match (base, override_val) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(over_map)) => {
            for (k, v) in over_map {
                json_deep_merge(
                    base_map.entry(k.clone()).or_insert(serde_json::Value::Null),
                    v,
                );
            }
        }
        (base, over) => {
            *base = over.clone();
        }
    }
}

/// Inject a settings override into `<worktree>/.claude/settings.local.json`.
/// Creates the `.claude/` directory and file if they don't exist.
/// Deep-merges the override into existing settings (objects merge recursively, scalars replace).
pub(crate) fn inject_settings_override(
    worktree_abs_path: &Path,
    settings: &serde_json::Value,
) -> Result<()> {
    let claude_dir = worktree_abs_path.join(".claude");
    std::fs::create_dir_all(&claude_dir)
        .with_context(|| format!("failed to create {}", claude_dir.display()))?;

    let settings_path = claude_dir.join("settings.local.json");
    let mut base: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("failed to read {}", settings_path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", settings_path.display()))?
    } else {
        serde_json::json!({})
    };

    json_deep_merge(&mut base, settings);

    let output = serde_json::to_string_pretty(&base)?;
    std::fs::write(&settings_path, output)
        .with_context(|| format!("failed to write {}", settings_path.display()))?;

    Ok(())
}

/// Focus an existing session pane.
pub fn focus_session(pane_id: &str) -> Result<()> {
    tmux::pane::focus_pane(pane_id)
}

/// Check if a session pane is still alive.
pub fn is_session_alive(pane_id: &str) -> bool {
    tmux::pane::pane_exists(pane_id)
}

/// Kill a session pane.
pub fn kill_session(pane_id: &str) -> Result<()> {
    tmux::pane::kill_pane(pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_builds_codex_resume_command() {
        let wt = Worktree::new(
            "wt-test".to_string(),
            std::path::PathBuf::from("/tmp/wt-test"),
            "wt/wt-test".to_string(),
            "main".to_string(),
            "HEAD".to_string(),
            crate::worktree::model::Lifecycle::Ephemeral,
        );
        let cfg = SessionConfig {
            provider: crate::session::provider::SessionProvider::Codex,
            command: String::new(),
            provider_args: vec!["--model".to_string(), "gpt-5-codex".to_string()],
            ..SessionConfig::default()
        };

        let cmd = build_provider_command(
            &wt,
            &cfg,
            Some("sess-123"),
            PermissionLevel::Normal,
            &PermissionsConfig::default(),
        );

        assert!(cmd.contains("codex"));
        assert!(cmd.contains("resume"));
        assert!(cmd.contains("sess-123"));
        assert!(cmd.contains("gpt-5-codex"));
    }

    #[test]
    fn provider_override_ignores_stale_builtin_command() {
        let wt = Worktree::new(
            "wt-test".to_string(),
            std::path::PathBuf::from("/tmp/wt-test"),
            "wt/wt-test".to_string(),
            "main".to_string(),
            "HEAD".to_string(),
            crate::worktree::model::Lifecycle::Ephemeral,
        );
        let cfg = SessionConfig {
            provider: crate::session::provider::SessionProvider::Codex,
            command: "claude".to_string(),
            ..SessionConfig::default()
        };

        let cmd = build_provider_command(
            &wt,
            &cfg,
            None,
            PermissionLevel::Normal,
            &PermissionsConfig::default(),
        );

        assert!(
            cmd.starts_with("codex"),
            "expected codex command, got: {cmd}"
        );
    }

    // --- json_deep_merge ---

    #[test]
    fn merge_disjoint_objects() {
        let mut base = json!({"a": 1});
        let over = json!({"b": 2});
        json_deep_merge(&mut base, &over);
        assert_eq!(base, json!({"a": 1, "b": 2}));
    }

    #[test]
    fn merge_overlapping_scalars_overrides() {
        let mut base = json!({"a": 1, "b": "old"});
        let over = json!({"b": "new"});
        json_deep_merge(&mut base, &over);
        assert_eq!(base, json!({"a": 1, "b": "new"}));
    }

    #[test]
    fn merge_nested_objects_recursively() {
        let mut base = json!({
            "sandbox": {"enabled": false, "timeout": 30}
        });
        let over = json!({
            "sandbox": {"enabled": true, "extra": "val"}
        });
        json_deep_merge(&mut base, &over);
        assert_eq!(
            base,
            json!({
                "sandbox": {"enabled": true, "timeout": 30, "extra": "val"}
            })
        );
    }

    #[test]
    fn merge_replaces_scalar_with_object() {
        let mut base = json!({"a": "string"});
        let over = json!({"a": {"nested": true}});
        json_deep_merge(&mut base, &over);
        assert_eq!(base, json!({"a": {"nested": true}}));
    }

    #[test]
    fn merge_replaces_object_with_scalar() {
        let mut base = json!({"a": {"nested": true}});
        let over = json!({"a": 42});
        json_deep_merge(&mut base, &over);
        assert_eq!(base, json!({"a": 42}));
    }

    #[test]
    fn merge_into_empty_base() {
        let mut base = json!({});
        let over = json!({"sandbox": {"enabled": true}});
        json_deep_merge(&mut base, &over);
        assert_eq!(base, json!({"sandbox": {"enabled": true}}));
    }

    #[test]
    fn merge_with_empty_override_is_noop() {
        let mut base = json!({"a": 1});
        let over = json!({});
        json_deep_merge(&mut base, &over);
        assert_eq!(base, json!({"a": 1}));
    }

    #[test]
    fn merge_deeply_nested_three_levels() {
        let mut base = json!({"l1": {"l2": {"l3": "old", "keep": true}}});
        let over = json!({"l1": {"l2": {"l3": "new", "add": 1}}});
        json_deep_merge(&mut base, &over);
        assert_eq!(
            base,
            json!({"l1": {"l2": {"l3": "new", "keep": true, "add": 1}}})
        );
    }

    // --- inject_settings_override ---

    #[test]
    fn inject_creates_file_when_none_exists() {
        let dir = tempfile::tempdir().unwrap();
        let settings = json!({"sandbox": {"enabled": true}});
        inject_settings_override(dir.path(), &settings).unwrap();

        let path = dir.path().join(".claude/settings.local.json");
        assert!(path.exists());
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["sandbox"]["enabled"], true);
    }

    #[test]
    fn inject_merges_into_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let path = claude_dir.join("settings.local.json");
        std::fs::write(
            &path,
            r#"{"existing": "value", "sandbox": {"timeout": 30}}"#,
        )
        .unwrap();

        let settings = json!({"sandbox": {"enabled": true}});
        inject_settings_override(dir.path(), &settings).unwrap();

        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        // Existing fields preserved
        assert_eq!(content["existing"], "value");
        assert_eq!(content["sandbox"]["timeout"], 30);
        // New field merged in
        assert_eq!(content["sandbox"]["enabled"], true);
    }

    #[test]
    fn inject_creates_dot_claude_directory() {
        let dir = tempfile::tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        assert!(!claude_dir.exists());

        inject_settings_override(dir.path(), &json!({"a": 1})).unwrap();
        assert!(claude_dir.exists());
        assert!(claude_dir.join("settings.local.json").exists());
    }

    #[test]
    fn inject_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let settings = json!({"sandbox": {"enabled": true}});

        inject_settings_override(dir.path(), &settings).unwrap();
        inject_settings_override(dir.path(), &settings).unwrap();

        let path = dir.path().join(".claude/settings.local.json");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content, json!({"sandbox": {"enabled": true}}));
    }

    #[test]
    fn inject_with_default_elevated_settings() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::config::model::PermissionsConfig::default();
        let settings = cfg
            .get(crate::config::model::PermissionLevel::Elevated)
            .settings_override
            .as_ref()
            .unwrap();

        inject_settings_override(dir.path(), settings).unwrap();

        let path = dir.path().join(".claude/settings.local.json");
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(content["sandbox"]["enabled"], true);
        assert_eq!(content["sandbox"]["autoAllowBashIfSandboxed"], true);
        assert_eq!(content["sandbox"]["allowUnsandboxedCommands"], false);
        assert_eq!(content["permissions"]["allow"], json!([]));
    }
}
