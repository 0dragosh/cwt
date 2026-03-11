use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::TcpListener;

/// A port allocation for a worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortAllocation {
    /// The worktree name this allocation belongs to.
    pub worktree_name: String,
    /// Map of logical name to allocated host port.
    /// e.g., "app" -> 3001, "db" -> 5433
    pub ports: HashMap<String, u16>,
}

impl PortAllocation {
    /// Get the primary port (the "app" port, or the first port).
    pub fn primary_port(&self) -> Option<u16> {
        self.ports
            .get("app")
            .copied()
            .or_else(|| self.ports.values().next().copied())
    }

    /// Generate environment variables for this allocation.
    pub fn env_vars(&self) -> Vec<(String, String)> {
        let mut vars = Vec::new();

        if let Some(primary) = self.primary_port() {
            vars.push(("CWT_PORT".to_string(), primary.to_string()));
        }

        for (name, port) in &self.ports {
            let env_key = format!("CWT_{}_PORT", name.to_uppercase().replace('-', "_"));
            vars.push((env_key, port.to_string()));
        }

        vars
    }

    /// Format as a human-readable port map line.
    /// e.g., "localhost:3001 (app), localhost:5433 (db)"
    pub fn format_port_map(&self) -> String {
        let mut entries: Vec<String> = self
            .ports
            .iter()
            .map(|(name, port)| format!("localhost:{} ({})", port, name))
            .collect();
        entries.sort();
        entries.join(", ")
    }
}

/// Port manager that tracks allocations across all worktrees.
pub struct PortManager {
    /// Base port for app allocations (e.g., 3000).
    pub app_base_port: u16,
    /// Base port for DB allocations (e.g., 5432).
    pub db_base_port: u16,
    /// Currently allocated ports by worktree name.
    allocations: HashMap<String, PortAllocation>,
}

impl PortManager {
    pub fn new(app_base_port: u16, db_base_port: u16) -> Self {
        Self {
            app_base_port,
            db_base_port,
            allocations: HashMap::new(),
        }
    }

    /// Create a port manager with default base ports.
    pub fn with_defaults() -> Self {
        Self::new(3000, 5432)
    }

    /// Rebuild the port manager from existing worktree port allocations.
    pub fn from_existing(allocations: Vec<PortAllocation>) -> Self {
        let mut manager = Self::with_defaults();
        for alloc in allocations {
            manager
                .allocations
                .insert(alloc.worktree_name.clone(), alloc);
        }
        manager
    }

    /// Get all current allocations.
    pub fn allocations(&self) -> &HashMap<String, PortAllocation> {
        &self.allocations
    }

    /// Allocate ports for a new worktree.
    /// `port_names` is a list of logical port names (e.g., ["app", "db"]).
    pub fn allocate(&mut self, worktree_name: &str, port_names: &[&str]) -> Result<PortAllocation> {
        if self.allocations.contains_key(worktree_name) {
            return Ok(self.allocations[worktree_name].clone());
        }

        let mut ports = HashMap::new();
        let used_ports = self.all_used_ports();

        for name in port_names {
            let base = if *name == "db" || name.contains("database") || name.contains("postgres") {
                self.db_base_port
            } else {
                self.app_base_port
            };

            let port = find_available_port(base, &used_ports)?;
            ports.insert(name.to_string(), port);
        }

        let allocation = PortAllocation {
            worktree_name: worktree_name.to_string(),
            ports,
        };

        self.allocations
            .insert(worktree_name.to_string(), allocation.clone());

        Ok(allocation)
    }

    /// Allocate a single "app" port for a worktree.
    pub fn allocate_app_port(&mut self, worktree_name: &str) -> Result<PortAllocation> {
        self.allocate(worktree_name, &["app"])
    }

    /// Release ports for a worktree.
    pub fn release(&mut self, worktree_name: &str) {
        self.allocations.remove(worktree_name);
    }

    /// Get the port allocation for a worktree, if any.
    pub fn get(&self, worktree_name: &str) -> Option<&PortAllocation> {
        self.allocations.get(worktree_name)
    }

    /// Get all currently used ports across all allocations.
    fn all_used_ports(&self) -> Vec<u16> {
        self.allocations
            .values()
            .flat_map(|alloc| alloc.ports.values().copied())
            .collect()
    }

    /// Format a global port map showing all allocations.
    pub fn format_global_port_map(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut sorted: Vec<_> = self.allocations.iter().collect();
        sorted.sort_by_key(|(name, _)| (*name).clone());

        for (wt_name, alloc) in sorted {
            for (port_name, port) in &alloc.ports {
                lines.push(format!("localhost:{} -> {} ({})", port, wt_name, port_name));
            }
        }

        lines.sort();
        lines
    }
}

/// Find an available port starting from `base`, avoiding `used` ports.
/// Tries base+1, base+2, etc., and verifies the port is actually free.
fn find_available_port(base: u16, used: &[u16]) -> Result<u16> {
    let mut port = base + 1;
    let max_attempts = 1000;

    for _ in 0..max_attempts {
        if port == 0 {
            break;
        }

        if !used.contains(&port) && is_port_free(port) {
            return Ok(port);
        }

        port = port.wrapping_add(1);
    }

    anyhow::bail!("could not find an available port starting from {}", base)
}

/// Check if a TCP port is free by attempting to bind to it.
fn is_port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}
