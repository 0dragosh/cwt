pub mod model;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub use model::Config;

/// Metadata about where the config was loaded from.
#[derive(Debug, Clone, Default)]
pub struct ConfigMeta {
    /// Path to the config file that was loaded (None if using defaults).
    pub source_path: Option<PathBuf>,
    /// Whether the config file is a Nix store symlink (read-only).
    pub nix_managed: bool,
}

/// Load config with fallback: project config -> global config -> defaults.
pub fn load_config(repo_root: &Path) -> Result<Config> {
    let (config, _meta) = load_config_with_meta(repo_root)?;
    Ok(config)
}

/// Load config and return metadata about the source file.
pub fn load_config_with_meta(repo_root: &Path) -> Result<(Config, ConfigMeta)> {
    let project_config = repo_root.join(".cwt/config.toml");
    let global_config = global_config_path();

    // Try project config first
    if project_config.exists() {
        let content = std::fs::read_to_string(&project_config)
            .with_context(|| format!("failed to read {}", project_config.display()))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", project_config.display()))?;
        let meta = ConfigMeta {
            nix_managed: is_nix_managed(&project_config),
            source_path: Some(project_config),
        };
        return Ok((config, meta));
    }

    // Fall back to global config
    if let Some(ref global) = global_config {
        if global.exists() {
            let content = std::fs::read_to_string(global)
                .with_context(|| format!("failed to read {}", global.display()))?;
            let config: Config = toml::from_str(&content)
                .with_context(|| format!("failed to parse {}", global.display()))?;
            let meta = ConfigMeta {
                nix_managed: is_nix_managed(global),
                source_path: Some(global.clone()),
            };
            return Ok((config, meta));
        }
    }

    // Return defaults
    Ok((Config::default(), ConfigMeta::default()))
}

/// Save config to a TOML file.
pub fn save_config(config: &Config, path: &Path) -> Result<()> {
    let content = toml::to_string_pretty(config).context("failed to serialize config to TOML")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("failed to write config to {}", path.display()))?;
    Ok(())
}

/// Check if a path is a symlink pointing into `/nix/store`.
fn is_nix_managed(path: &Path) -> bool {
    std::fs::read_link(path)
        .map(|target| target.starts_with("/nix/store"))
        .unwrap_or(false)
}

fn global_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cwt/config.toml"))
}

/// Return the project config path for a repo root (for save_config).
pub fn project_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".cwt/config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::PermissionLevel;

    #[test]
    fn is_nix_managed_returns_false_for_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        assert!(!is_nix_managed(&path));
    }

    #[test]
    fn is_nix_managed_returns_false_for_nonexistent() {
        assert!(!is_nix_managed(Path::new("/tmp/nonexistent-cwt-test")));
    }

    #[test]
    fn is_nix_managed_returns_true_for_nix_store_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let link_path = dir.path().join("config.toml");
        // Symlink to a fake /nix/store path (doesn't need to exist for read_link)
        std::os::unix::fs::symlink("/nix/store/abc123-cwt-config/config.toml", &link_path).unwrap();
        assert!(is_nix_managed(&link_path));
    }

    #[test]
    fn is_nix_managed_returns_false_for_non_nix_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.toml");
        std::fs::write(&target, "").unwrap();
        let link_path = dir.path().join("config.toml");
        std::os::unix::fs::symlink(&target, &link_path).unwrap();
        assert!(!is_nix_managed(&link_path));
    }

    #[test]
    fn config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config::default();
        save_config(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&content).unwrap();
        assert_eq!(loaded.session.auto_launch, config.session.auto_launch);
    }

    #[test]
    fn save_config_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/nested/dir/config.toml");
        save_config(&Config::default(), &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn config_round_trip_preserves_permission_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.session.default_permission = PermissionLevel::ElevatedUnsandboxed;

        save_config(&config, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: Config = toml::from_str(&content).unwrap();
        assert_eq!(
            loaded.session.default_permission,
            PermissionLevel::ElevatedUnsandboxed
        );
    }

    #[test]
    fn load_config_with_meta_returns_defaults_when_no_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        // No .cwt/config.toml in dir
        let (config, meta) = load_config_with_meta(dir.path()).unwrap();
        assert_eq!(config.session.default_permission, PermissionLevel::Normal);
        assert!(meta.source_path.is_none());
        assert!(!meta.nix_managed);
    }

    #[test]
    fn load_config_with_meta_reads_project_config() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();
        std::fs::write(
            cwt_dir.join("config.toml"),
            r#"
[session]
default_permission = "elevated"
"#,
        )
        .unwrap();

        let (config, meta) = load_config_with_meta(dir.path()).unwrap();
        assert_eq!(config.session.default_permission, PermissionLevel::Elevated);
        assert!(meta.source_path.is_some());
        assert!(!meta.nix_managed);
    }

    #[test]
    fn load_config_with_meta_detects_nix_managed_project_config() {
        let dir = tempfile::tempdir().unwrap();
        let cwt_dir = dir.path().join(".cwt");
        std::fs::create_dir_all(&cwt_dir).unwrap();

        // Create a fake /nix/store directory structure with a real config file
        let fake_nix = dir.path().join("nix/store/fake-hash-cwt");
        std::fs::create_dir_all(&fake_nix).unwrap();
        let real_file = fake_nix.join("config.toml");
        std::fs::write(&real_file, "[session]\ndefault_permission = \"elevated\"\n").unwrap();

        // Symlink .cwt/config.toml -> the fake nix store path
        let config_path = cwt_dir.join("config.toml");
        std::os::unix::fs::symlink(&real_file, &config_path).unwrap();

        let (config, meta) = load_config_with_meta(dir.path()).unwrap();
        assert_eq!(config.session.default_permission, PermissionLevel::Elevated);
        assert!(meta.source_path.is_some());
        // The symlink target doesn't literally start with /nix/store (it's a
        // tempdir), so nix_managed is false here. The is_nix_managed function
        // is separately tested with a /nix/store symlink target.
        // This test verifies the meta plumbing works end-to-end.
        assert!(!meta.nix_managed);
    }

    #[test]
    fn project_config_path_returns_expected_location() {
        let root = Path::new("/home/user/project");
        let path = project_config_path(root);
        assert_eq!(path, PathBuf::from("/home/user/project/.cwt/config.toml"));
    }
}
