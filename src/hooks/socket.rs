use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::hooks::event::HookEvent;

/// Compute the socket path for a given repo root.
/// Format: /tmp/cwt-<repo-hash>.sock
pub fn socket_path(repo_root: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    repo_root.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();
    PathBuf::from(format!("/tmp/cwt-{:016x}.sock", hash))
}

/// A listener that accepts connections on a Unix domain socket and parses
/// incoming JSON into HookEvent values, sending them through an mpsc channel.
pub struct HookSocketListener {
    path: PathBuf,
    /// Receiver end for the main thread to poll for events.
    pub receiver: mpsc::Receiver<HookEvent>,
    /// Handle to the background listener thread.
    _handle: Option<std::thread::JoinHandle<()>>,
}

impl HookSocketListener {
    /// Create and start the listener on a background thread.
    /// Returns immediately; events will be available on `self.receiver`.
    pub fn start(repo_root: &Path) -> Result<Self> {
        let path = socket_path(repo_root);

        // Clean up stale socket file if it exists
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove stale socket {}", path.display()))?;
        }

        let (tx, rx) = mpsc::channel();
        let listen_path = path.clone();

        let handle = std::thread::spawn(move || {
            if let Err(e) = run_listener(&listen_path, tx) {
                // Log but don't crash — hooks are optional
                eprintln!("cwt: hook socket listener error: {}", e);
            }
        });

        Ok(Self {
            path,
            receiver: rx,
            _handle: Some(handle),
        })
    }

    /// Drain all pending events from the channel (non-blocking).
    pub fn drain_events(&self) -> Vec<HookEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }

    /// Get the socket path (for display / hook installation).
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for HookSocketListener {
    fn drop(&mut self) {
        // Clean up the socket file
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Run the blocking listener loop.
fn run_listener(path: &Path, tx: mpsc::Sender<HookEvent>) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    let listener =
        UnixListener::bind(path).with_context(|| format!("failed to bind {}", path.display()))?;

    // Set a timeout so the listener thread can check if we should stop
    // (when the channel sender is dropped, sending will fail and we'll exit)
    listener
        .set_nonblocking(false)
        .context("failed to set socket blocking mode")?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    match line {
                        Ok(line) if line.trim().is_empty() => continue,
                        Ok(line) => match HookEvent::from_json(&line) {
                            Ok(event) => {
                                if tx.send(event).is_err() {
                                    // Receiver dropped, TUI is shutting down
                                    return Ok(());
                                }
                            }
                            Err(e) => {
                                eprintln!("cwt: failed to parse hook event: {} (line: {})", e, line);
                            }
                        },
                        Err(e) => {
                            eprintln!("cwt: error reading from hook socket: {}", e);
                            break; // Move on to next connection
                        }
                    }
                }
            }
            Err(e) => {
                // Check if the error is because the listener is being shut down
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                eprintln!("cwt: socket accept error: {}", e);
            }
        }
    }

    Ok(())
}
