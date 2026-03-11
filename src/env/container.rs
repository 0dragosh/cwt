use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// Container runtime backend.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerRuntime {
    Podman,
    Docker,
    #[default]
    None,
}

impl ContainerRuntime {
    /// Get the CLI command name for this runtime.
    pub fn cmd(&self) -> &str {
        match self {
            ContainerRuntime::Podman => "podman",
            ContainerRuntime::Docker => "docker",
            ContainerRuntime::None => "",
        }
    }
}

/// Status of a container associated with a worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerStatus {
    #[default]
    None,
    Building,
    Running,
    Stopped,
    Failed,
}

impl ContainerStatus {
    /// Icon for display in the worktree list.
    pub fn icon(&self) -> &'static str {
        match self {
            ContainerStatus::None => "",
            ContainerStatus::Building => "ctr:build",
            ContainerStatus::Running => "ctr:up",
            ContainerStatus::Stopped => "ctr:stop",
            ContainerStatus::Failed => "ctr:fail",
        }
    }

    /// Short label for the inspector.
    pub fn label(&self) -> &'static str {
        match self {
            ContainerStatus::None => "none",
            ContainerStatus::Building => "building",
            ContainerStatus::Running => "running",
            ContainerStatus::Stopped => "stopped",
            ContainerStatus::Failed => "failed",
        }
    }
}

/// Information about a container associated with a worktree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContainerInfo {
    /// Container ID (short hash).
    #[serde(default)]
    pub container_id: Option<String>,
    /// Container name (e.g., "cwt-feature-auth").
    #[serde(default)]
    pub container_name: Option<String>,
    /// Image used to build/run this container.
    #[serde(default)]
    pub image: Option<String>,
    /// Current container status.
    #[serde(default)]
    pub status: ContainerStatus,
    /// Runtime used (podman/docker).
    #[serde(default)]
    pub runtime: ContainerRuntime,
}

/// Detect the best available container runtime.
/// Prefers Podman (rootless) over Docker for NixOS compatibility.
pub fn detect_runtime() -> ContainerRuntime {
    if which::which("podman").is_ok() {
        ContainerRuntime::Podman
    } else if which::which("docker").is_ok() {
        ContainerRuntime::Docker
    } else {
        ContainerRuntime::None
    }
}

/// Check if a container runtime is available.
pub fn runtime_available() -> bool {
    detect_runtime() != ContainerRuntime::None
}

/// Build a container image from a Containerfile/Dockerfile.
/// Returns the image ID on success.
pub fn build_image(
    runtime: &ContainerRuntime,
    context_dir: &Path,
    containerfile: &str,
    image_tag: &str,
) -> Result<String> {
    if *runtime == ContainerRuntime::None {
        anyhow::bail!("no container runtime available");
    }

    let containerfile_path = if Path::new(containerfile).is_absolute() {
        containerfile.to_string()
    } else {
        context_dir
            .join(containerfile)
            .to_string_lossy()
            .to_string()
    };

    let output = Command::new(runtime.cmd())
        .args([
            "build",
            "-f",
            &containerfile_path,
            "-t",
            image_tag,
            ".",
        ])
        .current_dir(context_dir)
        .output()
        .with_context(|| format!("failed to run {} build", runtime.cmd()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} build failed: {}", runtime.cmd(), stderr.trim());
    }

    // Get the image ID
    let id_output = Command::new(runtime.cmd())
        .args(["images", "-q", image_tag])
        .output()
        .context("failed to get image ID")?;

    let image_id = String::from_utf8_lossy(&id_output.stdout)
        .trim()
        .to_string();

    Ok(image_id)
}

/// Run a container with the worktree mounted as a volume.
/// Returns the container ID on success.
pub fn run_container(
    runtime: &ContainerRuntime,
    image: &str,
    container_name: &str,
    worktree_path: &Path,
    env_vars: &[(String, String)],
    port_mappings: &[(u16, u16)],
) -> Result<String> {
    if *runtime == ContainerRuntime::None {
        anyhow::bail!("no container runtime available");
    }

    let worktree_str = worktree_path
        .to_str()
        .context("worktree path is not valid UTF-8")?;

    let mut args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        container_name.to_string(),
        "-v".to_string(),
        format!("{}:/workspace:Z", worktree_str),
        "-w".to_string(),
        "/workspace".to_string(),
    ];

    // Add environment variables
    for (key, value) in env_vars {
        args.push("-e".to_string());
        args.push(format!("{}={}", key, value));
    }

    // Add port mappings
    for (host_port, container_port) in port_mappings {
        args.push("-p".to_string());
        args.push(format!("{}:{}", host_port, container_port));
    }

    args.push(image.to_string());

    // Keep the container running with a sleep loop
    args.push("sleep".to_string());
    args.push("infinity".to_string());

    let output = Command::new(runtime.cmd())
        .args(&args)
        .output()
        .with_context(|| format!("failed to run {} run", runtime.cmd()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{} run failed: {}", runtime.cmd(), stderr.trim());
    }

    let container_id = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();

    // Truncate to short ID (12 chars)
    let short_id = if container_id.len() > 12 {
        container_id[..12].to_string()
    } else {
        container_id
    };

    Ok(short_id)
}

/// Execute a command inside a running container.
/// Returns (stdout, stderr, exit_code).
pub fn exec_in_container(
    runtime: &ContainerRuntime,
    container_id: &str,
    command: &[&str],
) -> Result<(String, String, i32)> {
    if *runtime == ContainerRuntime::None {
        anyhow::bail!("no container runtime available");
    }

    let mut args = vec!["exec", "-i", container_id];
    args.extend_from_slice(command);

    let output = Command::new(runtime.cmd())
        .args(&args)
        .output()
        .with_context(|| {
            format!(
                "failed to exec in container {}: {}",
                container_id,
                command.join(" ")
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    Ok((stdout, stderr, exit_code))
}

/// Stop a running container.
pub fn stop_container(runtime: &ContainerRuntime, container_id: &str) -> Result<()> {
    if *runtime == ContainerRuntime::None {
        return Ok(());
    }

    let output = Command::new(runtime.cmd())
        .args(["stop", container_id])
        .output()
        .with_context(|| format!("failed to stop container {}", container_id))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Don't error if container is already stopped
        if !stderr.contains("no such container") && !stderr.contains("not running") {
            anyhow::bail!("{} stop failed: {}", runtime.cmd(), stderr.trim());
        }
    }

    Ok(())
}

/// Remove a stopped container.
pub fn remove_container(runtime: &ContainerRuntime, container_id: &str) -> Result<()> {
    if *runtime == ContainerRuntime::None {
        return Ok(());
    }

    let output = Command::new(runtime.cmd())
        .args(["rm", "-f", container_id])
        .output()
        .with_context(|| format!("failed to remove container {}", container_id))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.contains("no such container") {
            anyhow::bail!("{} rm failed: {}", runtime.cmd(), stderr.trim());
        }
    }

    Ok(())
}

/// Check the status of a container.
pub fn inspect_container_status(
    runtime: &ContainerRuntime,
    container_id: &str,
) -> ContainerStatus {
    if *runtime == ContainerRuntime::None {
        return ContainerStatus::None;
    }

    let output = Command::new(runtime.cmd())
        .args([
            "inspect",
            "--format",
            "{{.State.Status}}",
            container_id,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
            match status.as_str() {
                "running" => ContainerStatus::Running,
                "exited" | "stopped" => ContainerStatus::Stopped,
                "created" | "configured" => ContainerStatus::Stopped,
                _ => ContainerStatus::Failed,
            }
        }
        _ => ContainerStatus::None,
    }
}

/// Get resource stats for a running container (CPU%, Memory).
/// Returns (cpu_percent, memory_bytes, memory_limit_bytes) or None.
pub fn container_stats(
    runtime: &ContainerRuntime,
    container_id: &str,
) -> Option<(f64, u64, u64)> {
    if *runtime == ContainerRuntime::None {
        return None;
    }

    let output = Command::new(runtime.cmd())
        .args([
            "stats",
            "--no-stream",
            "--format",
            "{{.CPUPerc}}\t{{.MemUsage}}",
            container_id,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.trim();
    let parts: Vec<&str> = line.split('\t').collect();

    if parts.len() < 2 {
        return None;
    }

    // Parse CPU percent (e.g., "5.23%")
    let cpu = parts[0]
        .trim_end_matches('%')
        .parse::<f64>()
        .unwrap_or(0.0);

    // Parse memory usage (e.g., "128.5MiB / 8GiB")
    let mem_parts: Vec<&str> = parts[1].split('/').collect();
    let mem_used = parse_memory_string(mem_parts.first().unwrap_or(&"0"));
    let mem_limit = parse_memory_string(mem_parts.get(1).unwrap_or(&"0"));

    Some((cpu, mem_used, mem_limit))
}

/// Parse a memory string like "128.5MiB" or "2GiB" into bytes.
fn parse_memory_string(s: &str) -> u64 {
    let s = s.trim();
    if s.ends_with("GiB") || s.ends_with("GB") {
        let num_str = s.trim_end_matches("GiB").trim_end_matches("GB").trim();
        let num: f64 = num_str.parse().unwrap_or(0.0);
        (num * 1024.0 * 1024.0 * 1024.0) as u64
    } else if s.ends_with("MiB") || s.ends_with("MB") {
        let num_str = s.trim_end_matches("MiB").trim_end_matches("MB").trim();
        let num: f64 = num_str.parse().unwrap_or(0.0);
        (num * 1024.0 * 1024.0) as u64
    } else if s.ends_with("KiB") || s.ends_with("KB") {
        let num_str = s.trim_end_matches("KiB").trim_end_matches("KB").trim();
        let num: f64 = num_str.parse().unwrap_or(0.0);
        (num * 1024.0) as u64
    } else if s.ends_with('B') {
        let num_str = s.trim_end_matches('B').trim();
        num_str.parse().unwrap_or(0)
    } else {
        s.parse().unwrap_or(0)
    }
}

/// Build and start a container for a worktree, given a Containerfile path.
/// This is the high-level entry point that combines build + run.
pub fn setup_container(
    worktree_name: &str,
    worktree_path: &Path,
    containerfile: &str,
    env_vars: &[(String, String)],
    port_mappings: &[(u16, u16)],
) -> Result<ContainerInfo> {
    let runtime = detect_runtime();
    if runtime == ContainerRuntime::None {
        anyhow::bail!(
            "no container runtime found (install podman or docker)"
        );
    }

    let image_tag = format!("cwt-{}", worktree_name);
    let container_name = format!("cwt-{}", worktree_name);

    // Build the image
    build_image(&runtime, worktree_path, containerfile, &image_tag)?;

    // Run the container
    let container_id = run_container(
        &runtime,
        &image_tag,
        &container_name,
        worktree_path,
        env_vars,
        port_mappings,
    )?;

    Ok(ContainerInfo {
        container_id: Some(container_id),
        container_name: Some(container_name),
        image: Some(image_tag),
        status: ContainerStatus::Running,
        runtime,
    })
}

/// Tear down a container for a worktree.
pub fn teardown_container(info: &ContainerInfo) -> Result<()> {
    if let Some(ref cid) = info.container_id {
        stop_container(&info.runtime, cid)?;
        remove_container(&info.runtime, cid)?;
    } else if let Some(ref name) = info.container_name {
        stop_container(&info.runtime, name)?;
        remove_container(&info.runtime, name)?;
    }
    Ok(())
}
