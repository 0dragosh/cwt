use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

use crate::env::container::{self, ContainerRuntime};

/// Resource usage for a single worktree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Disk usage in bytes for the worktree directory.
    pub disk_bytes: u64,
    /// Container CPU usage percentage (0.0-100.0+).
    pub container_cpu_percent: f64,
    /// Container memory usage in bytes.
    pub container_memory_bytes: u64,
    /// Container memory limit in bytes.
    pub container_memory_limit: u64,
}

impl ResourceUsage {
    /// Format disk usage as a human-readable string.
    pub fn format_disk(&self) -> String {
        format_bytes(self.disk_bytes)
    }

    /// Format container memory as a human-readable string.
    pub fn format_container_memory(&self) -> String {
        if self.container_memory_bytes == 0 {
            return String::new();
        }
        if self.container_memory_limit > 0 {
            format!(
                "{} / {}",
                format_bytes(self.container_memory_bytes),
                format_bytes(self.container_memory_limit)
            )
        } else {
            format_bytes(self.container_memory_bytes)
        }
    }

    /// Format container CPU as a percentage string.
    pub fn format_container_cpu(&self) -> String {
        if self.container_cpu_percent == 0.0 {
            return String::new();
        }
        format!("{:.1}%", self.container_cpu_percent)
    }
}

/// Warning about resource limits.
#[derive(Debug, Clone)]
pub struct ResourceWarning {
    pub worktree_name: String,
    pub message: String,
    pub severity: WarningSeverity,
}

/// Severity level for resource warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningSeverity {
    Info,
    Warning,
    Critical,
}

/// Disk usage thresholds.
const DISK_WARNING_BYTES: u64 = 1_073_741_824; // 1 GiB
const DISK_CRITICAL_BYTES: u64 = 5_368_709_120; // 5 GiB

/// Memory usage threshold (percentage of limit).
const MEMORY_WARNING_PERCENT: f64 = 80.0;
const MEMORY_CRITICAL_PERCENT: f64 = 95.0;

/// Calculate disk usage for a directory using `du`.
pub fn disk_usage(path: &Path) -> u64 {
    let output = Command::new("du").args(["-sb", "--"]).arg(path).output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Get full resource usage for a worktree.
pub fn get_resource_usage(
    worktree_path: &Path,
    container_id: Option<&str>,
    runtime: &ContainerRuntime,
) -> ResourceUsage {
    let disk_bytes = disk_usage(worktree_path);

    let (container_cpu_percent, container_memory_bytes, container_memory_limit) =
        if let Some(cid) = container_id {
            container::container_stats(runtime, cid).unwrap_or((0.0, 0, 0))
        } else {
            (0.0, 0, 0)
        };

    ResourceUsage {
        disk_bytes,
        container_cpu_percent,
        container_memory_bytes,
        container_memory_limit,
    }
}

/// Check for resource warnings across all worktrees.
pub fn check_warnings(worktrees: &[(String, ResourceUsage)]) -> Vec<ResourceWarning> {
    let mut warnings = Vec::new();

    for (name, usage) in worktrees {
        // Disk usage warnings
        if usage.disk_bytes >= DISK_CRITICAL_BYTES {
            warnings.push(ResourceWarning {
                worktree_name: name.clone(),
                message: format!("Disk usage critical: {} (>5 GiB)", usage.format_disk()),
                severity: WarningSeverity::Critical,
            });
        } else if usage.disk_bytes >= DISK_WARNING_BYTES {
            warnings.push(ResourceWarning {
                worktree_name: name.clone(),
                message: format!("Disk usage high: {} (>1 GiB)", usage.format_disk()),
                severity: WarningSeverity::Warning,
            });
        }

        // Container memory warnings
        if usage.container_memory_limit > 0 {
            let pct =
                (usage.container_memory_bytes as f64 / usage.container_memory_limit as f64) * 100.0;
            if pct >= MEMORY_CRITICAL_PERCENT {
                warnings.push(ResourceWarning {
                    worktree_name: name.clone(),
                    message: format!(
                        "Container memory critical: {} ({:.0}% of limit)",
                        usage.format_container_memory(),
                        pct,
                    ),
                    severity: WarningSeverity::Critical,
                });
            } else if pct >= MEMORY_WARNING_PERCENT {
                warnings.push(ResourceWarning {
                    worktree_name: name.clone(),
                    message: format!(
                        "Container memory high: {} ({:.0}% of limit)",
                        usage.format_container_memory(),
                        pct,
                    ),
                    severity: WarningSeverity::Warning,
                });
            }
        }
    }

    warnings
}

/// Format bytes as a human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GiB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KiB", bytes as f64 / 1_024.0)
    } else {
        format!("{} B", bytes)
    }
}
