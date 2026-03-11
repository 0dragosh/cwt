#!/usr/bin/env bash
# Bootstrap script for cwt — run this in an empty directory to scaffold the project
set -euo pipefail

echo "==> Initializing cwt project..."

# Init cargo project
cargo init --name cwt .

# Write Cargo.toml
cat > Cargo.toml << 'TOML'
[package]
name = "cwt"
version = "0.1.0"
edition = "2021"
description = "Claude Worktree Manager — TUI for managing git worktrees with Claude Code"
license = "MIT"

[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
clap = { version = "4", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4"] }
rand = "0.8"
dirs = "5"
which = "7"
anyhow = "1"
thiserror = "2"

[profile.release]
lto = true
strip = true
TOML

# Create directory structure
mkdir -p src/{ui/dialogs,worktree,session,git,tmux,config,state,hooks}

# Create mod.rs stubs
for dir in ui ui/dialogs worktree session git tmux config state hooks; do
    touch "src/$dir/mod.rs"
done

# Create .cwt directory for project state
mkdir -p .cwt/snapshots

# Create default config
cat > .cwt/config.toml << 'CFG'
[worktree]
dir = ".claude/worktrees"
max_ephemeral = 15
auto_name = true

[setup]
script = ""
timeout_secs = 120

[session]
auto_launch = true
claude_args = []

[handoff]
method = "patch"
warn_gitignore = true

[ui]
theme = "default"
show_diff_stat = true
CFG

# Create .gitignore
cat > .gitignore << 'GIT'
/target
.cwt/state.json
GIT

# Copy CLAUDE.md and PLAN.md if they exist alongside this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$SCRIPT_DIR/CLAUDE.md" ] && cp "$SCRIPT_DIR/CLAUDE.md" .
[ -f "$SCRIPT_DIR/PLAN.md" ] && cp "$SCRIPT_DIR/PLAN.md" .
[ -f "$SCRIPT_DIR/AGENTS.md" ] && cp "$SCRIPT_DIR/AGENTS.md" .

# Create Claude Code agents directory
mkdir -p .claude/agents

echo "==> Project scaffolded. Directory structure:"
find . -not -path './target/*' -not -path './.git/*' -type f | sort | head -40

echo ""
echo "==> Next steps:"
echo "  1. cargo check  (verify deps resolve)"
echo "  2. Copy agent definitions from AGENTS.md into .claude/agents/"
echo "  3. Start Phase 1: claude 'Implement Phase 1 from PLAN.md'"
