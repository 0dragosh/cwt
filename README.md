# cwt — Claude Worktree Manager

A TUI worktree manager for [Claude Code](https://docs.anthropic.com/en/docs/claude-code), built in Rust. Manage git worktrees purpose-built for parallel Claude Code sessions, all from a single terminal interface running inside tmux.

The worktree is the first-class primitive — sessions attach to worktrees, not the other way around.

```
Worktree (unit of work)
  ├── Branch (auto-created or user-specified)
  ├── Session (claude code instance, 0 or 1 active)
  ├── Lifecycle: ephemeral | permanent
  └── State: idle | running | waiting | done
```

## Requirements

- **git** (with worktree support)
- **tmux**
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) CLI (`claude`)

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

1. Navigate to a git repository
2. Start a tmux session (cwt requires tmux)
3. Run `cwt`

```sh
cd ~/my-project
tmux
cwt
```

## Keybindings

| Key | Action | Context |
|-----|--------|---------|
| `n` | New worktree | Global |
| `s` | Launch/resume Claude session | Worktree selected |
| `h` | Handoff changes (worktree <-> local) | Worktree selected |
| `p` | Promote to permanent | Ephemeral selected |
| `d` | Delete (with snapshot) | Worktree selected |
| `g` | Run garbage collection | Global |
| `r` | Restore from snapshot | Global |
| `Enter` | Open shell in worktree | Worktree selected |
| `j/k` `↓/↑` | Navigate list | Worktree list |
| `Tab` | Switch panel focus | Global |
| `/` | Filter/search worktrees | Worktree list |
| `?` | Help overlay | Global |
| `q` | Quit | Global |

## Concepts

### Two-Tier Worktree Model

- **Ephemeral** — cheap, disposable, one-task worktrees. Auto-GC'd when count exceeds the configured limit (default 15). A `.patch` snapshot is saved before deletion.
- **Permanent** — long-lived, explicitly promoted or created. Never auto-deleted.

### Handoff

Transfer changes between your main working directory and a worktree in either direction using `git diff`/`git apply` patches.

### Snapshots

Before any worktree is deleted, cwt saves a full diff as a `.patch` file under `.cwt/snapshots/` so no work is ever lost. Restore with `r`.

### Garbage Collection

Ephemeral worktrees are pruned when the count exceeds the threshold. Worktrees with running sessions, uncommitted changes, or unpushed commits are skipped.

## Configuration

cwt reads configuration from `.cwt/config.toml` (per-project) and `~/.config/cwt/config.toml` (global).

```toml
[worktree]
dir = ".claude/worktrees"        # worktree root (relative to repo root)
max_ephemeral = 15               # GC threshold
auto_name = true                 # generate slug names

[setup]
script = ""                      # run after worktree creation
timeout_secs = 120

[session]
auto_launch = true               # launch claude on worktree create
claude_args = []                 # extra args for claude invocation

[handoff]
method = "patch"                 # "patch" or "cherry-pick"
warn_gitignore = true

[ui]
theme = "default"
show_diff_stat = true            # show file change counts in list
```

## License

MIT
