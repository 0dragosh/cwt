use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Parsed devcontainer.json configuration.
/// Supports the subset of fields relevant to cwt.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevContainerConfig {
    /// Name of the dev container.
    #[serde(default)]
    pub name: Option<String>,

    /// Dockerfile path relative to .devcontainer/.
    #[serde(default, alias = "dockerFile")]
    pub dockerfile: Option<String>,

    /// Build context (defaults to ".").
    #[serde(default)]
    pub build: Option<DevContainerBuild>,

    /// Docker image to use (alternative to Dockerfile).
    #[serde(default)]
    pub image: Option<String>,

    /// Ports to forward.
    #[serde(default)]
    pub forward_ports: Vec<u16>,

    /// Environment variables to set in the container.
    #[serde(default)]
    pub container_env: HashMap<String, String>,

    /// Remote environment variables.
    #[serde(default)]
    pub remote_env: HashMap<String, String>,

    /// Post-create command (run once after container creation).
    #[serde(default)]
    pub post_create_command: Option<StringOrArray>,

    /// Post-start command (run each time the container starts).
    #[serde(default)]
    pub post_start_command: Option<StringOrArray>,

    /// On-create command.
    #[serde(default)]
    pub on_create_command: Option<StringOrArray>,

    /// Working directory inside the container.
    #[serde(default)]
    pub workspace_folder: Option<String>,

    /// Features to install.
    #[serde(default)]
    pub features: HashMap<String, serde_json::Value>,
}

/// Build configuration within devcontainer.json.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevContainerBuild {
    #[serde(default, alias = "dockerFile")]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub args: HashMap<String, String>,
}

/// A field that can be either a string or an array of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StringOrArray {
    String(String),
    Array(Vec<String>),
}

impl StringOrArray {
    /// Convert to a single shell command string.
    pub fn to_command(&self) -> String {
        match self {
            StringOrArray::String(s) => s.clone(),
            StringOrArray::Array(arr) => arr.join(" && "),
        }
    }
}

/// Detect if a worktree has a devcontainer configuration.
/// Checks for `.devcontainer/devcontainer.json` or `.devcontainer.json`.
pub fn find_devcontainer(worktree_path: &Path) -> Option<std::path::PathBuf> {
    let devcontainer_dir = worktree_path.join(".devcontainer/devcontainer.json");
    if devcontainer_dir.exists() {
        return Some(devcontainer_dir);
    }

    let devcontainer_root = worktree_path.join(".devcontainer.json");
    if devcontainer_root.exists() {
        return Some(devcontainer_root);
    }

    None
}

/// Detect if a worktree has a Containerfile or Dockerfile.
/// Returns the path to the first one found.
pub fn find_containerfile(worktree_path: &Path) -> Option<std::path::PathBuf> {
    let candidates = [
        ".devcontainer/Containerfile",
        ".devcontainer/Dockerfile",
        "Containerfile",
        "Dockerfile",
    ];

    for candidate in &candidates {
        let path = worktree_path.join(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Parse a devcontainer.json file.
pub fn parse_devcontainer(path: &Path) -> Result<DevContainerConfig> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    // Strip JSON comments (// and /* */) for compatibility
    let stripped = strip_json_comments(&content);

    let config: DevContainerConfig = serde_json::from_str(&stripped)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    Ok(config)
}

/// Strip single-line (//) and multi-line (/* */) comments from JSON.
fn strip_json_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;

    while i < chars.len() {
        if in_string {
            result.push(chars[i]);
            if chars[i] == '\\' && i + 1 < chars.len() {
                result.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if chars[i] == '"' {
                in_string = false;
            }
            i += 1;
        } else if chars[i] == '"' {
            in_string = true;
            result.push(chars[i]);
            i += 1;
        } else if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            // Single-line comment: skip to end of line
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            // Multi-line comment: skip to */
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            if i + 1 < chars.len() {
                i += 2; // skip */
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Resolve the Dockerfile/Containerfile to use from a devcontainer config.
/// Returns (containerfile_path, context_dir) relative to the devcontainer dir.
pub fn resolve_containerfile(
    config: &DevContainerConfig,
    devcontainer_path: &Path,
) -> Option<(String, std::path::PathBuf)> {
    let devcontainer_dir = devcontainer_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    // Check build.dockerfile first
    if let Some(ref build) = config.build {
        if let Some(ref dockerfile) = build.dockerfile {
            let context = build
                .context
                .as_deref()
                .unwrap_or(".");
            let context_path = devcontainer_dir.join(context);
            return Some((dockerfile.clone(), context_path));
        }
    }

    // Then check top-level dockerfile
    if let Some(ref dockerfile) = config.dockerfile {
        return Some((dockerfile.clone(), devcontainer_dir.to_path_buf()));
    }

    None
}

/// Extract environment variables from a devcontainer config.
pub fn extract_env_vars(config: &DevContainerConfig) -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = Vec::new();

    for (key, value) in &config.container_env {
        vars.push((key.clone(), value.clone()));
    }

    for (key, value) in &config.remote_env {
        if !vars.iter().any(|(k, _)| k == key) {
            vars.push((key.clone(), value.clone()));
        }
    }

    vars
}

/// Extract port mappings from a devcontainer config.
/// Returns (host_port, container_port) pairs (same port for both by default).
pub fn extract_port_mappings(config: &DevContainerConfig) -> Vec<(u16, u16)> {
    config
        .forward_ports
        .iter()
        .map(|&port| (port, port))
        .collect()
}
