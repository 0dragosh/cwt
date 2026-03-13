# cwt — Claude Worktree Manager

A terminal UI for running parallel [Claude Code](https://docs.anthropic.com/en/docs/claude-code) sessions in isolated git worktrees. Built in Rust, uses tmux for all interactive session management, and requires tmux to be installed.

> **The worktree is the first-class primitive** — sessions attach to worktrees, not the other way around.

```
Worktree (unit of work)
  |-- Branch (auto-created or user-specified)
  |-- Session (Claude Code instance, 0 or 1 active)
  |-- Lifecycle: ephemeral | permanent
  |-- State: idle | running | waiting | done | shipping
```

## Why cwt?

When using Claude Code on a real codebase, you often want to run multiple tasks in parallel — fix a bug, add a feature, write tests — without them stepping on each other. Git worktrees give you cheap, isolated copies of your repo. cwt manages the lifecycle of those worktrees and the Claude sessions inside them, all from a single TUI.

- **Spin up a worktree in seconds** — auto-named, auto-branched, ready to go
- **Never lose work** — every deletion saves a `.patch` snapshot first
- **Stay organized** — ephemeral worktrees auto-clean; permanent ones stick around
- **See everything at once** — two-panel TUI with live session status, diff stats, and transcript previews
- **Scale up** — dispatch tasks in bulk, import GitHub issues, broadcast prompts across sessions

## Requirements

- **git** (with worktree support)
- **tmux** (mandatory; interactive mode and session management depend on it)
- [**Claude Code**](https://docs.anthropic.com/en/docs/claude-code) CLI (`claude`)

Optional:

- **gh** ([GitHub CLI](https://cli.github.com/)) — for PR creation and CI status
- **podman** or **docker** — for per-worktree containers
- **ssh** — for remote worktrees

## Installation

### Cargo (from crates.io)

```sh
cargo install cwt
```

`cargo install` does not install tmux for you. Install `tmux` separately and make sure it is on your `PATH` before running `cwt`.

### Nix (recommended)

cwt provides a Nix flake with builds for Linux and macOS (x86_64 and aarch64). The Nix package includes `tmux` and `git` as runtime dependencies and wraps the binary so they are always on `PATH`.

```sh
# Run without installing
nix run github:0dragosh/cwt

# Install to your profile
nix profile install github:0dragosh/cwt
```

Add to a flake-based NixOS or home-manager configuration:

```nix
# flake.nix
{
  inputs.cwt.url = "github:0dragosh/cwt";

  # Option 1: use the overlay
  nixpkgs.overlays = [ cwt.overlays.default ];
  # then add pkgs.cwt to your packages

  # Option 2: reference the package directly
  environment.systemPackages = [ cwt.packages.${system}.default ];
}
```

### From source

```sh
git clone https://github.com/0dragosh/cwt.git
cd cwt
cargo build --release
# Binary at target/release/cwt — add it to your PATH
```

Make sure `git` is on your `PATH`, and make sure `tmux` is installed and on your `PATH`. `cwt` cannot run its interactive workflows without tmux.

## Quick Start

`tmux` is a hard dependency. If you launch `cwt` from a regular interactive shell, it will bootstrap into tmux automatically when possible, but tmux still must be installed locally.

```sh
# 1. Navigate to any git repo
cd ~/my-project

# 2. Launch the TUI (cwt will bootstrap into tmux if needed)
cwt

# 3. Press 'n' to create a worktree (Enter for auto-generated name)
# 4. Press 's' to launch a Claude session in it
# 5. Press 'Tab' to switch between the worktree list and inspector panels
```

Or use CLI commands directly:

```sh
cwt create my-feature --base main     # Create a worktree
cwt list                               # List all worktrees
cwt delete my-feature                  # Delete (saves a snapshot first)

# Dispatch parallel tasks — one worktree + session per task
cwt dispatch "implement auth" "add tests" "update docs"

# Import GitHub issues as worktrees
cwt import --github --limit 5

# Multi-repo mode
cwt add-repo ~/code/project-a
cwt add-repo ~/code/project-b
cwt forest                             # Launch forest TUI
cwt status                             # CLI summary across repos
```

## Features

### Worktree Management
- **Create** with auto-generated slug names or explicit names, from any base branch
- **Two-tier lifecycle**: ephemeral (auto-GC'd) and permanent (never auto-deleted)
- **Promote** ephemeral worktrees to permanent with a single keypress
- **Snapshots**: full diff saved as `.patch` before every deletion
- **Restore** previously deleted worktrees from their snapshots
- **Garbage collection**: prune old ephemeral worktrees, skipping those with running sessions, uncommitted changes, or unpushed commits
- **Setup scripts**: automatically run a script (e.g., `npm install`) after worktree creation

### TUI Interface
- **Two-panel layout**: worktree list (grouped by lifecycle) + inspector (details, diff stat, session info)
- **Fuzzy filter**: `/` to search/filter worktrees by name
- **Help overlay**: `?` for a full keybinding reference
- **Mouse support**: click to select, scroll to navigate
- **Status bar**: notification badges for waiting/done sessions

### tmux Session Management
- **Launch** Claude Code in a tmux pane attached to any worktree
- **Resume** previous sessions using Claude Code's `--resume` flag
- **Focus** an existing session pane with a single keypress
- **Open shell** in any worktree directory via a tmux pane
- Sessions survive TUI exit — closing cwt does not kill running sessions

### Handoff
- **Bidirectional** patch transfer between your main working directory and any worktree
- Direction picker: worktree-to-local or local-to-worktree
- Diff preview before applying
- Gitignore gap warnings for untracked files that won't transfer

### Hooks (Real-Time Claude Code Integration)
- **Unix domain socket** listener for sub-second event delivery
- Worktrees created by Claude Code outside cwt appear in the list within one second
- `cwt hooks install` patches `.claude/settings.json` and writes hook scripts to `.cwt/hooks/`

### Forest Mode (Multi-Repo)
- Register multiple repos with `cwt add-repo <path>`
- Three-panel TUI: repos | worktrees | inspector
- Aggregate session counts across all repos
- `cwt status` for a one-line CLI summary

### Agent Orchestration
- **Dispatch** multiple tasks in parallel: `cwt dispatch "task 1" "task 2" ...`
- **Import issues** from GitHub or Linear — creates worktrees and sessions per issue
- **Broadcast** a prompt to all running sessions simultaneously

### Ship Pipeline
- **Create PR** from a worktree with auto-generated body from session transcript
- **CI status tracking**: pass/fail/pending via `gh run list`
- **Ship it**: one-keypress macro to push, create PR, and mark as shipping

### Per-Worktree Containers
- Podman or Docker support (prefers Podman for rootless compatibility)
- Auto-detect `Containerfile`, `Dockerfile`, or `.devcontainer/devcontainer.json`
- Port management: auto-assign non-conflicting ports per worktree

### Remote Worktrees
- SSH-based remote host management
- Create and manage worktrees on remote machines
- Cross-machine handoff via patches
- Latency-aware polling with network status indicators

## Keybindings

### Worktree Actions

| Key | Action | Context |
|-----|--------|---------|
| `n` | New worktree | Global |
| `s` | Launch/resume Claude session | Worktree selected |
| `h` | Handoff changes (worktree <-> local) | Worktree selected |
| `p` | Promote to permanent | Ephemeral selected |
| `d` | Delete (with snapshot) | Worktree selected |
| `g` | Run garbage collection | Global |
| `r` | Restore from snapshot | Global |
| `Enter` | Open shell in worktree (tmux pane) | Worktree selected |

### Orchestration

| Key | Action | Context |
|-----|--------|---------|
| `t` | Dispatch tasks (multi-worktree) | Global |
| `b` | Broadcast prompt to all sessions | Global |

### Ship Pipeline

| Key | Action | Context |
|-----|--------|---------|
| `P` | Create PR (push + `gh pr create`) | Worktree selected |
| `S` | Ship it (push + PR + mark shipping) | Worktree selected |
| `c` | Open CI logs in browser | Worktree selected |

### Navigation

| Key | Action | Context |
|-----|--------|---------|
| `j` / `Down` | Move down / scroll inspector | Global |
| `k` / `Up` | Move up / scroll inspector | Global |
| `Tab` | Switch panel focus (forward) | Global |
| `Shift+Tab` | Switch panel focus (back) | Global |
| `R` | Switch to repo panel | Forest mode |
| `/` | Filter/search worktrees | Worktree list |
| `Esc` | Clear filter / close dialog | Global |
| `?` | Toggle help overlay | Global |
| `q` | Quit | Global |
| `Ctrl+C` | Force quit | Global |

## CLI Commands

| Command | Description |
|---------|-------------|
| `cwt` | Launch the TUI (default) |
| `cwt tui` | Launch the TUI (explicit) |
| `cwt list` | List all managed worktrees |
| `cwt create [name] --base <branch>` | Create a new worktree |
| `cwt create [name] --remote <host>` | Create on a remote host |
| `cwt delete <name>` | Delete a worktree (saves snapshot) |
| `cwt promote <name>` | Promote ephemeral to permanent |
| `cwt gc [--execute]` | Preview/run garbage collection |
| `cwt hooks install` | Install Claude Code hook scripts |
| `cwt hooks uninstall` | Remove hook scripts |
| `cwt hooks status` | Show hook and socket status |
| `cwt dispatch "task" ...` | Dispatch parallel tasks |
| `cwt import --github [--limit N]` | Import GitHub issues as worktrees |
| `cwt import --linear [--limit N]` | Import Linear issues as worktrees |
| `cwt add-repo <path>` | Register a repo for forest mode |
| `cwt forest` | Launch forest (multi-repo) TUI |
| `cwt status` | Summary of all repos and sessions |

## Configuration

cwt reads configuration from `.cwt/config.toml` (per-project) and `~/.config/cwt/config.toml` (global). Forest mode uses `~/.config/cwt/forest.toml`.

```toml
[worktree]
dir = ".claude/worktrees"        # worktree root (relative to repo root)
max_ephemeral = 15               # GC threshold
auto_name = true                 # generate slug names when no name given

[setup]
script = ""                      # path to setup script (relative to repo root)
timeout_secs = 120               # setup script timeout

[session]
auto_launch = true               # launch Claude on worktree create
claude_args = []                 # extra args for claude invocation

[handoff]
method = "patch"                 # "patch" or "cherry-pick"
warn_gitignore = true            # warn about .gitignore gaps

[ui]
theme = "default"                # color theme
show_diff_stat = true            # show file change counts in list

[container]
enabled = false                  # enable container support
runtime = "auto"                 # "podman", "docker", or "auto"
auto_ports = true                # auto-assign ports per worktree

# Remote hosts (one [[remote]] block per host)
[[remote]]
name = "build-server"
host = "build.example.com"
user = "dev"
worktree_dir = "/data/worktrees"
```

## Architecture

```
src/
  main.rs          # CLI parsing, TUI bootstrap, startup checks
  app.rs           # App state, event loop, keybinding dispatch, rendering
  config/          # TOML config loading (project + global fallback)
  state/           # JSON state persistence (.cwt/state.json)
  git/             # Git worktree, branch, and diff operations
  worktree/        # Worktree CRUD, handoff, snapshots, setup, slug generation
  session/         # Claude session launcher, tracker, transcript parser
  tmux/            # tmux pane create/focus/kill/send-keys
  hooks/           # Unix socket listener, hook events, script installer
  forest/          # Multi-repo config, global index
  orchestration/   # Task dispatch, issue import, broadcast, dashboard
  ship/            # PR creation, CI status, ship pipeline
  env/             # Containers (Podman/Docker), devcontainer, ports, resources
  remote/          # SSH host management, remote sessions, cross-machine sync
  ui/              # ratatui widgets: layout, list, inspector, dialogs, theme
```

## Troubleshooting

**cwt says "tmux is required"**
`tmux` is a mandatory runtime dependency. Install it first, then run `cwt` again. If you launch `cwt` from a normal interactive shell, it will bootstrap into tmux automatically when possible.

**Worktrees don't appear after Claude Code creates them**
Run `cwt hooks install` to set up the real-time hook integration. Without hooks, cwt discovers worktrees on periodic refresh (every few seconds).

**`gh` commands fail (PR creation, CI status)**
Make sure the [GitHub CLI](https://cli.github.com/) is installed and authenticated (`gh auth login`).

**Sessions show "idle" even though Claude is running**
cwt detects session status by parsing `~/.claude/projects/` transcripts. If the path hash doesn't match, status won't update. Restarting cwt re-scans the project directory.

**GC skipped a worktree I expected it to prune**
GC never prunes worktrees with running sessions, uncommitted changes, or unpushed commits. Check `cwt list` for details.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code conventions, and how to submit changes.

## License

[MIT](LICENSE)
