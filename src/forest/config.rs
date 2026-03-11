use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single repo entry in forest.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    /// Absolute path to the repo root.
    pub path: PathBuf,
    /// Human-readable name (defaults to directory name).
    pub name: String,
}

/// The forest configuration: a list of registered repos.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ForestConfig {
    #[serde(default)]
    pub repo: Vec<RepoEntry>,
}

/// Return the path to ~/.config/cwt/forest.toml.
pub fn forest_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cwt").join("forest.toml"))
}

/// Load the forest config from ~/.config/cwt/forest.toml.
/// Returns an empty config if the file doesn't exist.
pub fn load_forest_config() -> Result<ForestConfig> {
    let path = match forest_config_path() {
        Some(p) => p,
        None => return Ok(ForestConfig::default()),
    };

    if !path.exists() {
        return Ok(ForestConfig::default());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config: ForestConfig =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config)
}

/// Save the forest config to ~/.config/cwt/forest.toml.
pub fn save_forest_config(config: &ForestConfig) -> Result<()> {
    let path = forest_config_path().context("unable to determine config directory")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let content = toml::to_string_pretty(config).context("failed to serialize forest config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Add a repo to the forest config. Returns true if it was newly added, false if already present.
pub fn add_repo(path: &Path) -> Result<bool> {
    let abs_path = std::fs::canonicalize(path)
        .with_context(|| format!("failed to resolve path {}", path.display()))?;

    // Verify it's a git repo
    crate::git::commands::repo_root(&abs_path)
        .with_context(|| format!("{} is not a git repository", abs_path.display()))?;

    let mut config = load_forest_config()?;

    // Check if already registered
    if config.repo.iter().any(|r| r.path == abs_path) {
        return Ok(false);
    }

    let name = abs_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| abs_path.to_string_lossy().to_string());

    config.repo.push(RepoEntry {
        path: abs_path,
        name,
    });

    save_forest_config(&config)?;
    Ok(true)
}
