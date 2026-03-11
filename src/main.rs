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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let repo_root = git::commands::repo_root(&cwd).context("not in a git repository")?;
    let config = config::load_config(&repo_root)?;

    let manager = Manager::new(repo_root, config);

    match cli.command {
        None | Some(Commands::Tui) => run_tui(manager)?,
        Some(Commands::List) => cmd_list(&manager)?,
        Some(Commands::Create { name, base, carry }) => {
            cmd_create(&manager, name.as_deref(), &base, carry)?
        }
        Some(Commands::Delete { name }) => cmd_delete(&manager, &name)?,
        Some(Commands::Promote { name }) => cmd_promote(&manager, &name)?,
        Some(Commands::Gc { execute }) => cmd_gc(&manager, execute)?,
    }

    Ok(())
}

fn run_tui(manager: Manager) -> Result<()> {
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

        // Refresh session statuses periodically (every ~20 ticks = ~5 seconds)
        tick_count = tick_count.wrapping_add(1);
        if tick_count.is_multiple_of(20) {
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
