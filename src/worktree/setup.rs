use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::config::model::SetupConfig;

/// Result of running a setup script.
#[derive(Debug)]
pub struct SetupResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Run the configured setup script in the given worktree directory.
/// Returns None if no setup script is configured.
pub fn run_setup_script(worktree_path: &Path, config: &SetupConfig) -> Option<Result<SetupResult>> {
    if config.script.is_empty() {
        return None;
    }

    Some(execute_script(
        worktree_path,
        &config.script,
        config.timeout_secs,
    ))
}

fn execute_script(worktree_path: &Path, script: &str, timeout_secs: u64) -> Result<SetupResult> {
    // Resolve script path relative to worktree and validate it doesn't escape
    let script_path = if Path::new(script).is_absolute() {
        std::path::PathBuf::from(script)
    } else {
        let joined = worktree_path.join(script);
        // Canonicalize to resolve ".." and validate the path stays within worktree
        let canonical = joined.canonicalize().unwrap_or(joined.clone());
        let wt_canonical = worktree_path
            .canonicalize()
            .unwrap_or(worktree_path.to_path_buf());
        if !canonical.starts_with(&wt_canonical) {
            anyhow::bail!(
                "setup script path '{}' resolves outside the worktree directory",
                script
            );
        }
        canonical
    };

    // Use Command::new("sh") with the script path as an argument (not via -c)
    // to avoid command injection while preserving compatibility with scripts
    // that lack a shebang or executable permission.
    let mut child = Command::new("sh")
        .arg(&script_path)
        .current_dir(worktree_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run setup script: {}", script))?;

    let timeout = Duration::from_secs(timeout_secs);

    // Wait with timeout
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Ok(SetupResult {
                    success: status.success(),
                    stdout,
                    stderr,
                    timed_out: false,
                });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(SetupResult {
                        success: false,
                        stdout: String::new(),
                        stderr: format!("setup script timed out after {}s", timeout_secs),
                        timed_out: true,
                    });
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(e).context("failed to wait for setup script");
            }
        }
    }
}
