# cwt -- Claude Worktree Manager

A TUI worktree manager for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), built in Rust. Manage git worktrees purpose-built for parallel Claude Code sessions, all from a single terminal interface running inside tmux.

The worktree is the first-class primitive -- sessions attach to worktrees, not the other way around.

```
Worktree (unit of work)
  |-- Branch (auto-created or user-specified)
  |-- Session (Claude Code instance, 0 or 1 active)
  |-- Lifecycle: ephemeral | permanent
  |-- State: idle | running | waiting | done | shipping
  |-- Optional: container, port allocation, remote host
```

## Features

### Worktree Management

- **Create** worktrees with auto-generated slug names or explicit names, from any base branch
- **Two-tier lifecycle**: ephemeral (auto-GC'd) and permanent (never auto-deleted)
- **Promote** ephemeral worktrees to permanent with a single keypress
- **Snapshots**: full diff saved as `.patch` before every deletion -- no work is ever lost
- **Restore** previously deleted worktrees from their snapshots
- **Garbage collection**: prune ephemeral worktrees beyond the configured limit, oldest first, skipping those with running sessions, uncommitted changes, or unpushed commits
- **Setup scripts**: automatically run a script (e.g., `npm install`, `cargo build`) after worktree creation

### TUI Interface

- **Two-panel layout**: worktree list (grouped by lifecycle) + inspector (details, diff stat, session info, PR/CI status, container status, resource usage)
- **Fuzzy filter**: `/` to search/filter worktrees by name
- **Help overlay**: `?` for a full keybinding reference
- **Mouse support**: click to select worktrees, scroll to navigate
- **Scrollable inspector**: `j`/`k` when inspector is focused
- **Status bar**: notification badges for waiting/done sessions, aggregate dashboard stats
- **Theme**: dark-terminal-friendly color scheme

### tmux Session Management

- **Launch** Claude Code in a tmux pane attached to any worktree
- **Resume** previous sessions using Claude Code's `--resume` flag
- **Focus** an existing session pane with a single keypress
- **Open shell** in any worktree directory via a tmux pane
- Sessions survive TUI exit -- closing cwt does not kill running sessions

### Handoff

- **Bidirectional** patch transfer between your main working directory and any worktree
- Direction picker: worktree-to-local or local-to-worktree
- Diff preview before applying
- Gitignore gap warnings for untracked files that won't transfer

### Hooks (Real-Time Claude Code Integration)

- **Unix domain socket** listener at `/tmp/cwt-<repo-hash>.sock` for sub-second event delivery
- Hook events: `WorktreeCreated`, `WorktreeRemoved`, `SessionStopped`, `SessionNotification`, `SubagentStopped`
- `cwt hooks install` patches `.claude/settings.json` and writes hook scripts to `.cwt/hooks/`
- Worktrees created by Claude Code outside cwt appear in the list within one second

### Forest Mode (Multi-Repo)

- Register multiple git repos with `cwt add-repo <path>`
- **Three-panel TUI** in forest mode: repos | worktrees | inspector
- **Global dashboard**: aggregate session counts across all repos
- `cwt status` for a one-line CLI summary of all repos and active sessions
- Per-repo state with a global index at `~/.config/cwt/index.json`

### Agent Orchestration

- **Dispatch** multiple tasks in parallel: `cwt dispatch "task 1" "task 2" ...` creates a worktree per task and launches Claude with `--prompt`
- **Import issues** from GitHub (`cwt import --github`) or Linear (`cwt import --linear`) -- creates worktrees and sessions per issue
- **Broadcast** a prompt to all running sessions simultaneously via tmux `send-keys`
- **Aggregate dashboard**: token usage, cost totals, message counts, per-session progress

### Ship Pipeline

- **Create PR** (`P`): commit staged changes, push branch, create PR via `gh pr create` with auto-generated body from session transcript
- **Ship it** (`S`): one-keypress macro -- push, create PR, mark worktree as "shipping"
- **PR status tracking**: draft / open / approved / merged / closed, polled periodically
- **CI status**: GitHub Actions pass/fail/pending via `gh run list`
- **Open CI logs** (`c`): opens the latest CI run in your browser
- **Auto-cleanup**: on merge, worktree is flagged for deletion

### Per-Worktree Containers

- **Podman or Docker** support (prefers Podman for rootless/NixOS compatibility)
- Auto-detect `Containerfile`, `Dockerfile`, or `.devcontainer/devcontainer.json`
- Worktree mounted as `/workspace` volume inside the container
- **Port management**: auto-assign non-conflicting ports per worktree (`CWT_PORT`, `CWT_APP_PORT`, `CWT_DB_PORT` env vars)
- **Resource tracking**: disk usage, container CPU/memory, with configurable warning thresholds
- Falls back to bare setup scripts when no container runtime is available

### Remote Worktrees

- **SSH-based** remote host management with configurable connection details
- Create worktrees on remote machines: `cwt create --remote <host> <name>`
- Sessions run in remote tmux via SSH, focusable from local TUI
- **Cross-machine handoff**: generate patch locally, apply on remote (or vice versa)
- **Latency-aware polling**: remote statuses checked infrequently to avoid network overhead
- Network status indicators: connected (with latency), disconnected, unknown
- Remote worktrees displayed with `[host]` label in the TUI

## Requirements

- **git** (with worktree support)
- **tmux** (for session management)
- [**Claude Code**](https://docs.anthropic.com/en/docs/claude-code) CLI (`claude`)

Optional:

- **gh** ([GitHub CLI](https://cli.github.com/)) -- for ship pipeline (PR creation, CI status)
- **podman** or **docker** -- for per-worktree containers
- **ssh** -- for remote worktrees

## Installation

### Using Nix (recommended)

cwt provides a Nix flake with builds for Linux and macOS (x86_64 and aarch64).

**Run directly without installing:**

```sh
nix run github:0dragosh/cwt
```

**Install to your profile:**

```sh
nix profile install github:0dragosh/cwt
```

**Add to a flake-based NixOS or home-manager configuration:**

```nix
# flake.nix
{
  inputs.cwt.url = "github:0dragosh/cwt";

  # Option 1: use the overlay
  nixpkgs.overlays = [ cwt.overlays.default ];
  # then add `pkgs.cwt` to your packages

  # Option 2: reference the package directly
  environment.systemPackages = [ cwt.packages.${system}.default ];
}
```

**Enter the development shell:**

```sh
nix develop
```

This gives you a full Rust toolchain with `rust-analyzer`, `cargo-watch`, `cargo-edit`, plus `git` and `tmux`.

### From source with Cargo

```sh
git clone https://github.com/0dragosh/cwt.git
cd cwt
cargo build --release
# Binary is at target/release/cwt
```

Make sure `git` and `tmux` are available on your `PATH`. The Nix package wraps the binary to include these automatically.

## Quick Start

```sh
# Navigate to a git repo and start a tmux session
cd ~/my-project
tmux

# Launch the TUI
cwt

# Or use CLI commands directly:
cwt create my-feature --base main     # Create a worktree
cwt list                               # List all worktrees
cwt delete my-feature                  # Delete (with snapshot)

# Dispatch parallel tasks
cwt dispatch "implement auth" "add tests" "update docs"

# Import GitHub issues as worktrees
cwt import --github --limit 5

# Multi-repo mode
cwt add-repo ~/code/project-a
cwt add-repo ~/code/project-b
cwt forest                             # Launch forest TUI
cwt status                             # CLI summary
```

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
| Mouse click | Select worktree | Worktree list |

## CLI Commands

| Command | Description |
|---------|-------------|
| `cwt` | Launch the TUI (default) |
| `cwt tui` | Launch the TUI (explicit) |
| `cwt list` | List all managed worktrees |
| `cwt create [name] --base <branch>` | Create a new worktree |
| `cwt create [name] --remote <host>` | Create a worktree on a remote host |
| `cwt delete <name>` | Delete a worktree (saves snapshot) |
| `cwt promote <name>` | Promote ephemeral to permanent |
| `cwt gc [--execute]` | Preview/run garbage collection |
| `cwt hooks install` | Install Claude Code hook scripts |
| `cwt hooks uninstall` | Remove Claude Code hook scripts |
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
auto_launch = true               # launch claude on worktree create
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
containerfile = ""               # path to Containerfile (overrides auto-detect)
auto_ports = true                # auto-assign ports per worktree
app_base_port = 3000             # starting port for app allocations
db_base_port = 5432              # starting port for db allocations
port_names = ["app"]             # port names to auto-allocate
disk_warning_bytes = 1073741824  # 1 GiB disk usage warning
track_resources = false          # periodic resource tracking

# Remote hosts (one [[remote]] block per host)
[[remote]]
name = "fenrir"
host = "fenrir.local"
user = "d"
worktree_dir = "/data/worktrees"
port = 22
identity_file = ""
```

Forest mode configuration (`~/.config/cwt/forest.toml`):

```toml
[[repo]]
path = "/home/user/code/project-a"
name = "project-a"

[[repo]]
path = "/home/user/code/project-b"
name = "project-b"
```

## State

cwt persists worktree metadata in `.cwt/state.json` per project. Snapshots are saved as `.patch` files under `.cwt/snapshots/`. The state file tracks worktree names, branches, lifecycle, session IDs, tmux panes, PR/CI status, container info, port allocations, and remote host assignments.

## Architecture

```
src/
  main.rs                   # CLI parsing, TUI bootstrap, startup checks
  app.rs                    # App state, event loop, keybinding dispatch
  config/                   # TOML config loading (project + global)
  state/                    # JSON state persistence (.cwt/state.json)
  git/                      # Git worktree, branch, and diff operations
  worktree/                 # Worktree CRUD, handoff, snapshots, setup, slug gen
  session/                  # Claude session launcher, tracker, transcript parser
  tmux/                     # tmux pane create/focus/kill/send-keys
  hooks/                    # Unix socket listener, hook events, script installer
  forest/                   # Multi-repo config, global index
  orchestration/            # Task dispatch, issue import, broadcast, dashboard
  ship/                     # PR creation, CI status, ship pipeline
  env/                      # Containers (Podman/Docker), devcontainer, ports, resources
  remote/                   # SSH host management, remote sessions, cross-machine sync
  ui/                       # ratatui widgets: layout, worktree list, inspector,
                            #   repo list, status bar, theme, help, dialogs
```

## License

MIT
