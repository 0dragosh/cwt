# cwt — Provider (Claude, Codex, or Pi) Worktree Manager

A TUI worktree manager for the provider (Claude, Codex, or Pi). The worktree is the first-class primitive — sessions attach to worktrees, not the other way around.

## Project Overview

`cwt` is a Rust TUI (ratatui + crossterm) that manages git worktrees purpose-built for parallel provider (Claude, Codex, or Pi) sessions. It runs inside tmux and manages panes for each active session.

### Core Mental Model

```
Worktree (unit of work)
  |-- Branch (auto-created or user-specified)
  |-- Session (provider instance, 0 or 1 active)
  |-- Lifecycle: ephemeral | permanent
  |-- State: idle | running | waiting | done | shipping
  |-- Optional: container, port allocation, remote host
```

### Two-Tier Worktree Model

- **Ephemeral**: cheap, disposable, one-task worktrees. Auto-GC'd when count exceeds `max_ephemeral` (default 15). A `.patch` snapshot is saved before deletion.
- **Permanent**: long-lived, explicitly promoted or created. Never auto-deleted. Can have multiple sessions over their lifetime.

## Tech Stack

- **Language**: Rust (2021 edition, stable)
- **TUI**: ratatui 0.30 + crossterm 0.28
- **Terminal multiplexing**: tmux (required dependency)
- **Async runtime**: tokio (full features)
- **State**: JSON file per project at `.cwt/state.json`
- **Config**: TOML at `.cwt/config.toml` (project) and `~/.config/cwt/config.toml` (global)

## Architecture

```
src/
  main.rs                   # Entry point, CLI parsing, tmux bootstrap
  app.rs                    # App state, event loop, keybinding dispatch, rendering
  config/
    mod.rs                  # Config loading with project -> global -> default fallback
    model.rs                # Config structs, TOML deserialization, defaults
  state/
    mod.rs
    store.rs                # JSON state persistence (.cwt/state.json)
  git/
    mod.rs
    commands.rs             # git worktree add/remove/list, branch ops
    branch.rs               # List branches, current branch, remote tracking
    diff.rs                 # git diff --stat parsing
  worktree/
    mod.rs
    manager.rs              # CRUD: create, delete, promote, list, gc
    model.rs                # Worktree struct, Lifecycle enum, serialization
    handoff.rs              # Bidirectional local <-> worktree patch transfer
    snapshot.rs             # Save diff-as-patch before delete
    setup.rs                # Run setup scripts on worktree creation
    slug.rs                 # Auto-generate slug names (adj-noun-hex)
  session/
    mod.rs
    launcher.rs             # Launch provider in tmux pane
    tracker.rs              # Parse provider session directories for session status
    transcript.rs           # Read last N messages from session JSONL
  tmux/
    mod.rs
    pane.rs                 # Create/focus/kill tmux panes, session detection
  hooks/
    mod.rs
    event.rs                # HookEvent enum + JSON serde
    socket.rs               # Unix domain socket listener (async, tokio)
    install.rs              # Generate hook scripts + patch settings.json
  forest/
    mod.rs
    config.rs               # Forest config at ~/.config/cwt/forest.toml
    index.rs                # Index of all repos + stats aggregation
  orchestration/
    mod.rs
    dispatch.rs             # Parallel task dispatch into worktrees
    import.rs               # GitHub/Linear issue importing
    broadcast.rs            # Broadcast messages to all sessions via tmux send-keys
    dashboard.rs            # Aggregate stats (tokens, cost, message counts)
  ship/
    mod.rs
    pipeline.rs             # Pipeline stages
    pr.rs                   # GitHub PR creation via gh CLI
    ci.rs                   # CI status polling via gh run list
  env/
    mod.rs
    container.rs            # Dev container introspection & setup (Podman/Docker)
    devcontainer.rs         # .devcontainer.json parsing
    ports.rs                # Port allocation manager
    resources.rs            # CPU/memory/disk monitoring
  remote/
    mod.rs
    host.rs                 # RemoteHost config & SSH operations
    session.rs              # Remote session tracking
    sync.rs                 # Repo sync to remote hosts
  ui/
    mod.rs
    layout.rs               # Two-panel layout, top bar, status
    worktree_list.rs        # Left panel: worktree list with status icons
    inspector.rs            # Right panel: details, diff, session info
    repo_list.rs            # Forest mode: repo picker
    status_bar.rs           # Top bar with notification badges
    help.rs                 # Help overlay
    theme.rs                # Colors, symbols, borders
    dialogs/
      mod.rs
      create.rs             # New worktree dialog (name, branch, options)
      delete.rs             # Delete confirmation
      handoff.rs            # Handoff direction + diff preview
      gc.rs                 # GC preview modal
      restore.rs            # Restore from snapshot
      dispatch.rs           # Task dispatch dialog
      broadcast.rs          # Broadcast message dialog
      ship.rs               # PR/ship dialog
```

## Key Behaviors

### Worktree Creation Flow
1. User presses `n`
2. Dialog: enter name (or press Enter for auto-generated slug like `bold-oak-a3f2`)
   - Quick-create: pressing Enter on an empty name field creates immediately with all defaults
3. Dialog: pick base branch (fuzzy finder over local + remote branches)
4. Dialog: carry local changes? (only shown if working dir is dirty)
5. `git worktree add .claude/worktrees/<name> -b wt/<name> <base>`
6. If carry changes: `git stash` -> apply in worktree -> pop stash in local
7. If setup script configured: run it in the worktree directory
8. New worktree is auto-selected in the list
9. If `auto_launch` enabled: session starts automatically
10. Register in `.cwt/state.json` as ephemeral

### Handoff Flow
1. User selects worktree, presses `h`
2. Show diff preview of worktree changes vs its base
3. Confirm direction: "Apply worktree changes to local" or "Send local changes to worktree"
4. For WT->Local: generate patch with `git diff` in worktree, apply with `git apply` in local
5. For Local->WT: same in reverse
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
- Launch: `tmux split-window -h -t <session> "cd <worktree-path> && <provider-command>"`
- Focus: `tmux select-pane -t cwt:<name>`
- Status check: `tmux list-panes -F '#{pane_title} #{pane_current_command}'`

### Session Transcript Preview
- Claude/Codex store sessions at `~/.claude/projects/<path-hash>/`.
- Pi stores sessions at `~/.pi/agent/sessions/--<path-hash>--/`.
- Each session is a `.jsonl` file with conversation turns
- Parse the last 2-3 assistant messages for the "Last msg" preview
- Show token count / cost if available in the transcript

### GC (Garbage Collection)
- Triggered manually with `g` or on startup if over limit
- Sort ephemeral worktrees by last activity (session timestamp or file mtime)
- Skip worktrees with: running sessions, uncommitted changes, unpushed commits
- Snapshot remaining, then delete oldest until under `max_ephemeral`
- Show preview of what will be pruned before executing

### Hooks (Provider Integration)

cwt integrates with providers for session management, and currently integrates with Claude hooks for real-time state sync.
Pi and Codex do not get hook installation or hook-driven worktree import in this phase.

#### Communication Path
```
Provider hook fires
  -> runs .cwt/hooks/<event>.sh
    -> reads JSON from stdin (provider event payload)
    -> transforms to cwt event format
    -> writes JSON to Unix socket /tmp/cwt-<repo-hash>.sock
      -> cwt TUI event loop reads from socket
        -> updates state + re-renders
```

#### Hook Events
| Provider Hook | cwt Event | Effect |
|---|---|---|
| WorktreeCreate | WorktreeCreated | New worktree appears in list |
| WorktreeRemove | WorktreeRemoved | Worktree removed from list |
| Stop | SessionStopped | Status flips to done |
| Notification | SessionNotification | Status flips to waiting |
| SubagentStop | SubagentStopped | Update subagent tracking |

Unix sockets are used instead of file polling for sub-second latency and clean async I/O integration with tokio.

## Keybindings

| Key | Action | Context |
|-----|--------|---------|
| `n` | New worktree (Enter to quick-create) | Global |
| `s` | Launch/resume provider session | Worktree selected |
| `Enter` | Open shell in worktree (tmux pane) | Worktree selected |
| `h` | Handoff | Worktree selected |
| `p` | Promote to permanent | Ephemeral selected |
| `d` | Delete (with snapshot) | Worktree selected |
| `g` | Run GC | Global |
| `r` | Restore from snapshot | Global |
| `t` | Dispatch tasks | Global |
| `b` | Broadcast prompt | Global |
| `P` | Create PR | Worktree selected |
| `S` | Ship it | Worktree selected |
| `c` | Open CI logs | Worktree selected |
| `j/k` or arrows | Navigate list / scroll inspector | Global |
| `Tab` | Switch panel focus | Global |
| `/` | Filter/search worktrees | Worktree list |
| `?` | Help overlay | Global |
| `o` | Cycle session provider (Claude/Codex/Pi) at runtime | Global |
| `O` | Save current provider as default | Global |
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
auto_launch = true               # launch provider on worktree create
provider = "claude"              # "claude" | "codex" | "pi"
provider_args = []               # extra args for provider invocation

[handoff]
method = "patch"                 # "patch" or "cherry-pick"
warn_gitignore = true            # warn about .gitignore gaps

[ui]
theme = "default"
show_diff_stat = true            # show file change counts in list

[container]
enabled = false                  # enable container support
runtime = "auto"                 # "podman", "docker", or "auto"
auto_ports = true                # auto-assign ports per worktree

[[remote]]
name = "build-server"
host = "build.example.com"
user = "dev"
worktree_dir = "/data/worktrees"
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
      "patch_file": ".cwt/snapshots/bugfix-old-20260311.patch",
      "base_commit": "e5f6g7h8",
      "deleted_at": "2026-03-11T09:00:00Z"
    }
  ]
}
```

## Development Notes

- Always run `cargo clippy` before committing
- Use `anyhow` for application errors, `thiserror` for library-style errors in the core modules
- All git operations go through `src/git/commands.rs` — never shell out to git directly from other modules
- All tmux operations go through `src/tmux/pane.rs`
- The TUI event loop is async (tokio) — keep it non-blocking
- Test git operations against a temp repo created in tests (use `tempfile` crate)
- Integration tests are in `tests/integration.rs`
