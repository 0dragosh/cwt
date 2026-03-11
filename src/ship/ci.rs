use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::ship::pr::CiStatus;

/// Fetch the CI (GitHub Actions) status for a branch.
/// Uses `gh run list` to check the latest workflow run status.
pub fn fetch_ci_status(repo_path: &Path, branch: &str) -> CiStatus {
    let output = Command::new("gh")
        .args([
            "run",
            "list",
            "--branch",
            branch,
            "--limit",
            "1",
            "--json",
            "status,conclusion",
        ])
        .current_dir(repo_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return CiStatus::None,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return CiStatus::None,
    };

    let Some(run) = json.first() else {
        return CiStatus::None;
    };

    let status = run
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let conclusion = run
        .get("conclusion")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match status {
        "completed" => match conclusion {
            "success" => CiStatus::Passed,
            "failure" | "timed_out" | "cancelled" => CiStatus::Failed,
            _ => CiStatus::None,
        },
        "in_progress" | "queued" | "waiting" | "requested" | "pending" => CiStatus::Pending,
        _ => CiStatus::None,
    }
}

/// Open CI logs in the default browser for the latest run on a branch.
pub fn open_ci_logs(repo_path: &Path, branch: &str) -> Result<()> {
    // Get the latest run URL
    let output = Command::new("gh")
        .args([
            "run",
            "list",
            "--branch",
            branch,
            "--limit",
            "1",
            "--json",
            "url",
        ])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        anyhow::bail!("failed to list CI runs");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Vec<serde_json::Value> = serde_json::from_str(&stdout)?;

    if let Some(run) = json.first() {
        if let Some(url) = run.get("url").and_then(|v| v.as_str()) {
            // Use xdg-open / open to open in browser
            let open_cmd = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };

            Command::new(open_cmd)
                .arg(url)
                .spawn()
                .ok();

            return Ok(());
        }
    }

    anyhow::bail!("no CI runs found for branch '{}'", branch);
}
