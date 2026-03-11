pub mod model;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub use model::Config;

/// Load config with fallback: project config → global config → defaults.
pub fn load_config(repo_root: &Path) -> Result<Config> {
    let project_config = repo_root.join(".cwt/config.toml");
    let global_config = global_config_path();

    // Try project config first
    if project_config.exists() {
        let content = std::fs::read_to_string(&project_config)
            .with_context(|| format!("failed to read {}", project_config.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", project_config.display()))?;
        return Ok(config);
    }

    // Fall back to global config
    if let Some(ref global) = global_config {
        if global.exists() {
            let content = std::fs::read_to_string(global)
                .with_context(|| format!("failed to read {}", global.display()))?;
            let config: Config = toml::from_str(&content)
                .with_context(|| format!("failed to parse {}", global.display()))?;
            return Ok(config);
        }
    }

    // Return defaults
    Ok(Config::default())
}

fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cwt/config.toml"))
}
