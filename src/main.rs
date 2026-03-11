#![allow(dead_code)]

mod app;
mod config;
mod git;
mod hooks;
mod session;
mod state;
mod tmux;
mod ui;
mod worktree;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use crate::worktree::Manager;

#[derive(Parser)]
#[command(name = "cwt", about = "Claude Worktree Manager — TUI for managing git worktrees with Claude Code")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all managed worktrees
    List,
    /// Create a new worktree
    Create {
        /// Name for the worktree (auto-generated if omitted)
        name: Option<String>,
        /// Base branch to create from
        #[arg(short, long, default_value = "main")]
        base: String,
        /// Carry uncommitted local changes into the new worktree
        #[arg(short, long)]
        carry: bool,
    },
    /// Delete a worktree (saves a snapshot first)
    Delete {
        /// Name of the worktree to delete
        name: String,
    },
    /// Promote an ephemeral worktree to permanent
    Promote {
        /// Name of the worktree to promote
        name: String,
    },
    /// Run garbage collection on ephemeral worktrees
    Gc {
        /// Actually delete (without this flag, just preview)
        #[arg(long)]
        execute: bool,
    },
    /// Launch the interactive TUI
    Tui,
    /// Manage Claude Code hooks integration
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
}

#[derive(Subcommand)]
enum HooksAction {
    /// Install cwt hooks into the Claude Code configuration
    Install,
    /// Remove cwt hooks from the Claude Code configuration
    Uninstall,
    /// Show hook status and socket path
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // For hooks status, we don't strictly need to be in a git repo
    // but for consistency we still check.

    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // Check if we're in a git repo — provide friendly error for TUI mode
    let repo_root = match git::commands::repo_root(&cwd) {
        Ok(root) => root,
        Err(_) => {
            eprintln!("error: not in a git repository");
            eprintln!();
            eprintln!("cwt manages git worktrees and must be run from within a git repository.");
            eprintln!("  cd /path/to/your/repo && cwt");
            std::process::exit(1);
        }
    };

    let config = config::load_config(&repo_root)?;
    let manager = Manager::new(repo_root.clone(), config);

    match cli.command {
        None | Some(Commands::Tui) => run_tui(manager)?,
        Some(Commands::List) => cmd_list(&manager)?,
        Some(Commands::Create { name, base, carry }) => {
            cmd_create(&manager, name.as_deref(), &base, carry)?
        }
        Some(Commands::Delete { name }) => cmd_delete(&manager, &name)?,
        Some(Commands::Promote { name }) => cmd_promote(&manager, &name)?,
        Some(Commands::Gc { execute }) => cmd_gc(&manager, execute)?,
        Some(Commands::Hooks { action }) => cmd_hooks(&repo_root, action)?,
    }

    Ok(())
}

fn run_tui(manager: Manager) -> Result<()> {
    // Startup checks
    startup_checks()?;

    // Set up terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // Start hook socket listener
    let hook_listener = hooks::socket::HookSocketListener::start(&manager.repo_root)
        .ok(); // Non-fatal if socket fails

    // Create app
    let mut app = app::App::new(manager)?;

    // Refresh counter for periodic status updates
    let mut tick_count: u32 = 0;

    // Main loop
    loop {
        terminal.draw(|f| app.draw(f))?;
        app.tick()?;

        if app.should_quit {
            break;
        }

        // Process hook events (non-blocking)
        if let Some(ref listener) = hook_listener {
            let events = listener.drain_events();
            for event in events {
                app.handle_hook_event(event);
            }
        }

        // Refresh session statuses periodically (every ~4 ticks = ~1 second)
        tick_count = tick_count.wrapping_add(1);
        if tick_count.is_multiple_of(4) {
            app.refresh();
            app.update_inspector();
        }
    }

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // hook_listener is dropped here, which cleans up the socket file

    Ok(())
}

/// Perform startup checks and provide friendly error messages.
fn startup_checks() -> Result<()> {
    // Check that git is available
    if which::which("git").is_err() {
        eprintln!("error: git not found on PATH");
        eprintln!();
        eprintln!("cwt requires git for worktree management.");
        eprintln!("  Install git: https://git-scm.com/downloads");
        std::process::exit(1);
    }

    // Check that tmux is available (warn but don't block)
    if which::which("tmux").is_err() {
        eprintln!("warning: tmux not found on PATH");
        eprintln!("  Session launching requires tmux.");
        eprintln!("  Install tmux: https://github.com/tmux/tmux/wiki/Installing");
        eprintln!();
    }

    // Check that claude is available (warn but don't block)
    if which::which("claude").is_err() {
        eprintln!("warning: claude not found on PATH");
        eprintln!("  Session launching requires Claude Code CLI.");
        eprintln!("  Install: https://docs.anthropic.com/en/docs/claude-code");
        eprintln!();
    }

    Ok(())
}

fn cmd_list(manager: &Manager) -> Result<()> {
    let worktrees = manager.list()?;

    if worktrees.is_empty() {
        println!("No managed worktrees.");
        return Ok(());
    }

    println!("{:<20} {:<12} {:<25} {:<10}", "NAME", "LIFECYCLE", "BRANCH", "STATUS");
    println!("{}", "-".repeat(70));

    for wt in &worktrees {
        let lifecycle = match wt.lifecycle {
            worktree::Lifecycle::Ephemeral => "ephemeral",
            worktree::Lifecycle::Permanent => "permanent",
        };
        let status = format!("{:?}", wt.status).to_lowercase();
        println!("{:<20} {:<12} {:<25} {:<10}", wt.name, lifecycle, wt.branch, status);
    }

    println!("\n{} worktree(s)", worktrees.len());
    Ok(())
}

fn cmd_create(manager: &Manager, name: Option<&str>, base: &str, carry: bool) -> Result<()> {
    let wt = manager.create(name, base, carry)?;
    let abs_path = manager.worktree_abs_path(&wt);
    println!("Created worktree '{}'", wt.name);
    println!("  Path:   {}", abs_path.display());
    println!("  Branch: {}", wt.branch);
    println!("  Base:   {} ({})", wt.base_branch, &wt.base_commit[..8.min(wt.base_commit.len())]);
    Ok(())
}

fn cmd_delete(manager: &Manager, name: &str) -> Result<()> {
    manager.delete(name)?;
    println!("Deleted worktree '{}' (snapshot saved)", name);
    Ok(())
}

fn cmd_promote(manager: &Manager, name: &str) -> Result<()> {
    manager.promote(name)?;
    println!("Promoted '{}' to permanent", name);
    Ok(())
}

fn cmd_gc(manager: &Manager, execute: bool) -> Result<()> {
    let to_prune = manager.gc_preview()?;

    if to_prune.is_empty() {
        println!("Nothing to GC — ephemeral count is within limit.");
        return Ok(());
    }

    println!("Worktrees to prune ({}):", to_prune.len());
    for name in &to_prune {
        println!("  - {}", name);
    }

    if execute {
        let deleted = manager.gc_execute(&to_prune)?;
        println!("\nDeleted {} worktree(s) (snapshots saved).", deleted.len());
    } else {
        println!("\nDry run — use --execute to actually delete.");
    }

    Ok(())
}

fn cmd_hooks(repo_root: &std::path::Path, action: HooksAction) -> Result<()> {
    match action {
        HooksAction::Install => {
            hooks::install::install_hooks(repo_root)?;
        }
        HooksAction::Uninstall => {
            hooks::install::uninstall_hooks(repo_root)?;
        }
        HooksAction::Status => {
            let sock_path = hooks::socket::socket_path(repo_root);
            let hooks_dir = repo_root.join(".cwt/hooks");
            let settings_path = repo_root.join(".claude/settings.json");

            println!("Hook status for {}", repo_root.display());
            println!();

            // Socket
            if sock_path.exists() {
                println!("  Socket: {} (active)", sock_path.display());
            } else {
                println!("  Socket: {} (inactive — TUI not running)", sock_path.display());
            }

            // Hook scripts
            if hooks_dir.exists() {
                let scripts: Vec<_> = std::fs::read_dir(&hooks_dir)?
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with("cwt-")
                    })
                    .collect();
                if scripts.is_empty() {
                    println!("  Hooks:  not installed");
                } else {
                    println!("  Hooks:  {} script(s) in {}", scripts.len(), hooks_dir.display());
                    for s in &scripts {
                        println!("          - {}", s.file_name().to_string_lossy());
                    }
                }
            } else {
                println!("  Hooks:  not installed");
            }

            // Settings.json
            if settings_path.exists() {
                let content = std::fs::read_to_string(&settings_path)?;
                let has_cwt = content.contains("cwt-");
                if has_cwt {
                    println!("  Claude: settings.json patched");
                } else {
                    println!("  Claude: settings.json exists but no cwt hooks registered");
                }
            } else {
                println!("  Claude: no .claude/settings.json found");
            }
        }
    }

    Ok(())
}
