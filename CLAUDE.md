# cwt — Claude Worktree Manager

A TUI worktree manager for Claude Code, modeled after the Codex desktop app's worktree system. The worktree is the first-class primitive — sessions attach to worktrees, not the other way around.

## Project Overview

`cwt` is a Rust TUI (ratatui + crossterm) that manages git worktrees purpose-built for parallel Claude Code sessions. It runs inside tmux and manages panes for each active session.

### Core Mental Model

```
Worktree (unit of work)
  ├── Branch (auto-created or user-specified)
  ├── Session (claude code instance, 0 or 1 active)
  ├── Lifecycle: ephemeral | permanent
  └── State: idle | running | waiting | done
```

### Two-Tier Worktree Model

- **Ephemeral**: cheap, disposable, one-task worktrees. Auto-GC'd when count exceeds `max_ephemeral` (default 15). A `.patch` snapshot is saved before deletion.
- **Permanent**: long-lived, explicitly promoted or created. Never auto-deleted. Can have multiple sessions over their lifetime.

## Tech Stack

- **Language**: Rust (2021 edition, stable)
- **TUI**: ratatui + crossterm
- **Terminal multiplexing**: tmux (required dependency)
- **State**: JSON file per project at `.cwt/state.json`
- **Config**: TOML at `.cwt/config.toml` (project) and `~/.config/cwt/config.toml` (global)

## Key Dependencies

```toml
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
rand = "0.8"                    # for slug generation
dirs = "5"                      # XDG paths
which = "7"                     # find tmux, claude, git
anyhow = "1"
thiserror = "2"
```

## Architecture

```
src/
├── main.rs                  # Entry point, CLI parsing, app bootstrap
├── app.rs                   # Top-level App state + update loop
├── ui/
│   ├── mod.rs
│   ├── layout.rs            # Two-panel layout (worktree list + inspector)
│   ├── worktree_list.rs     # Left panel: worktree list with status icons
│   ├── inspector.rs         # Right panel: details, diff, session info
│   ├── dialogs/
│   │   ├── mod.rs
│   │   ├── create.rs        # New worktree dialog (name, branch, options)
│   │   ├── handoff.rs       # Handoff confirmation with diff preview
│   │   ├── delete.rs        # Delete confirmation
│   │   └── gc.rs            # GC preview (what will be pruned)
│   ├── branch_picker.rs     # Fuzzy branch selector
│   └── theme.rs             # Colors, symbols, borders
├── worktree/
│   ├── mod.rs
│   ├── manager.rs           # CRUD: create, delete, promote, list, gc
│   ├── model.rs             # Worktree struct, lifecycle enum, serialization
│   ├── snapshot.rs          # Save diff-as-patch before delete
│   ├── handoff.rs           # Bidirectional local ↔ worktree transfer
│   └── setup.rs             # Run setup scripts on worktree creation
├── session/
│   ├── mod.rs
│   ├── launcher.rs          # Launch claude in tmux pane
│   ├── tracker.rs           # Parse ~/.claude/ for session status
│   └── transcript.rs        # Read last N messages from session transcript
├── git/
│   ├── mod.rs
│   ├── commands.rs          # git worktree add/remove/list, branch ops
│   ├── diff.rs              # git diff --stat, git diff for inspector
│   └── branch.rs            # List branches, current branch, remote tracking
├── tmux/
│   ├── mod.rs
│   └── pane.rs              # Create/focus/kill tmux panes for sessions
├── config/
│   ├── mod.rs
│   └── model.rs             # Config structs, TOML loading, defaults
├── hooks/
│   ├── mod.rs
│   └── claude.rs            # Generate .claude/settings.json hook entries
└── state/
    ├── mod.rs
    └── store.rs             # JSON state persistence (.cwt/state.json)
```

## Key Behaviors

### Worktree Creation Flow
1. User presses `n`
2. Dialog: enter name (or press Enter for auto-generated slug like `bold-oak-a3f2`)
3. Dialog: pick base branch (fuzzy finder over local + remote branches)
4. Dialog: carry local changes? (only shown if working dir is dirty)
5. `git worktree add .claude/worktrees/<name> -b wt/<name> <base>`
6. If carry changes: `git stash` → apply in worktree → pop stash in local
7. If setup script configured: run it in the worktree directory
8. If `auto_launch` enabled: open Claude Code session in tmux pane
9. Register in `.cwt/state.json` as ephemeral

### Handoff Flow (Worktree → Local)
1. User selects worktree, presses `h`
2. Show diff preview of worktree changes vs its base
3. Confirm direction: "Apply worktree changes to local" or "Send local changes to worktree"
4. For WT→Local: generate patch with `git diff` in worktree, apply with `git apply` in local
5. For Local→WT: same in reverse
6. Warn if `.gitignore`d files exist that won't transfer

### Snapshot Before Delete
1. User presses `d` on a worktree
2. Generate `git diff` of all changes (committed + uncommitted)
3. Save as `.cwt/snapshots/<name>-<timestamp>.patch`
4. Also save metadata: base commit, branch name, creation time
5. Confirm deletion
6. `git worktree remove --force <path>` + `git branch -D wt/<name>`

### Session Launching (tmux)
- Each worktree session runs in a tmux pane within the current tmux session
- Pane naming: `cwt:<worktree-name>`
- Launch: `tmux split-window -h -t <session> "cd <worktree-path> && claude"`
- Focus: `tmux select-pane -t cwt:<name>`
- Status check: `tmux list-panes -F '#{pane_title} #{pane_current_command}'`

### Session Transcript Preview
- Claude Code stores sessions at `~/.claude/projects/<path-hash>/`
- Each session is a `.jsonl` file with conversation turns
- Parse the last 2-3 assistant messages for the "Last msg" preview
- Show token count / cost if available in the transcript

### GC (Garbage Collection)
- Triggered manually with `g` or on startup if over limit
- Sort ephemeral worktrees by last activity (session timestamp or file mtime)
- Skip worktrees with: running sessions, uncommitted changes, unpushed commits
- Snapshot remaining, then delete oldest until under `max_ephemeral`
- Show preview of what will be pruned before executing

## Keybindings

| Key | Action | Context |
|-----|--------|---------|
| `n` | New worktree | Global |
| `s` | Launch/resume session | Worktree selected |
| `h` | Handoff | Worktree selected |
| `p` | Promote to permanent | Ephemeral selected |
| `d` | Delete (with snapshot) | Worktree selected |
| `g` | Run GC | Global |
| `r` | Restore from snapshot | Global |
| `Enter` | Open shell in worktree | Worktree selected |
| `j/k` or `↓/↑` | Navigate list | Worktree list |
| `Tab` | Switch panel focus | Global |
| `/` | Filter/search worktrees | Worktree list |
| `?` | Help overlay | Global |
| `q` | Quit | Global |

## Config Format

```toml
# .cwt/config.toml
[worktree]
dir = ".claude/worktrees"        # worktree root (relative to repo root)
max_ephemeral = 15               # GC threshold
auto_name = true                 # generate slug names

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
theme = "default"                # future: theme support
show_diff_stat = true            # show file change counts in list
```

## State Format

```json
{
  "version": 1,
  "repo_root": "/home/user/project",
  "worktrees": {
    "feature-auth": {
      "name": "feature-auth",
      "path": ".claude/worktrees/feature-auth",
      "branch": "wt/feature-auth",
      "base_branch": "main",
      "base_commit": "a1b2c3d4",
      "lifecycle": "ephemeral",
      "created_at": "2026-03-11T10:30:00Z",
      "last_session_id": "session-9f2a3b",
      "tmux_pane": "cwt:feature-auth"
    }
  },
  "snapshots": [
    {
      "name": "bugfix-old",
      "patch_file": "~/.cwt/snapshots/bugfix-old-20260311.patch",
      "base_commit": "e5f6g7h8",
      "deleted_at": "2026-03-11T09:00:00Z"
    }
  ]
}
```

## Non-Goals for v0.1

- Multi-repo / forest mode (single repo only)
- Claude Code hooks auto-registration (manual for now)
- Cloud/remote worktrees
- Agent teams integration
- PR creation from worktrees (use claude or gh directly)

## Development Notes

- Always run `cargo clippy` before committing
- Use `anyhow` for application errors, `thiserror` for library-style errors in the core modules
- All git operations go through `src/git/commands.rs` — never shell out to git directly from other modules
- All tmux operations go through `src/tmux/pane.rs`
- The TUI event loop is async (tokio) so we can poll for session status changes without blocking the UI
- Test git operations against a temp repo created in tests (use `tempfile` crate)
