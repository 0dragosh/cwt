# cwt Build Plan

Phased implementation plan for Claude Code sessions. Each phase is scoped to roughly one focused work session.

## Phase 1 — Project Scaffold + Git Worktree CRUD

**Goal**: Rust project compiles, can list/create/delete git worktrees from the CLI (no TUI yet).

### Tasks

1. **`cargo init cwt`** — set up Cargo.toml with all dependencies from CLAUDE.md
2. **CLI entry point** (`src/main.rs`)
   - Use clap with subcommands: `cwt list`, `cwt create <name>`, `cwt delete <name>`, `cwt tui`
   - The `tui` subcommand will launch the interactive TUI (added in Phase 2)
   - Other subcommands are for scripting / testing during development
3. **Git module** (`src/git/`)
   - `commands.rs`: wrap `git worktree list --porcelain`, `git worktree add`, `git worktree remove`
   - `branch.rs`: list local+remote branches, get current branch, get HEAD commit
   - `diff.rs`: `git diff --stat` and `git diff` for a given worktree path
   - All commands use `std::process::Command` with proper error handling
   - Parse `--porcelain` output into structured data
4. **Worktree model** (`src/worktree/model.rs`)
   - `Worktree` struct: name, path, branch, base_branch, base_commit, lifecycle (Ephemeral|Permanent), created_at, last_session_id, tmux_pane
   - `Lifecycle` enum with serde serialization
   - `WorktreeStatus` enum: Idle, Running, Waiting, Done
5. **Worktree manager** (`src/worktree/manager.rs`)
   - `list()` → merge git porcelain output with state.json data
   - `create(name, base_branch, carry_changes)` → full creation flow
   - `delete(name)` → snapshot + remove
   - `promote(name)` → flip lifecycle in state
6. **Slug generator** — random two-word slugs (adjective-noun-hex4) for auto-naming
7. **State store** (`src/state/store.rs`)
   - Load/save `.cwt/state.json`
   - Create `.cwt/` dir if not exists
   - Merge state with actual git worktree list (handle drift)
8. **Config loader** (`src/config/`)
   - Load `.cwt/config.toml` with defaults for everything
   - Global fallback at `~/.config/cwt/config.toml`
9. **Snapshot module** (`src/worktree/snapshot.rs`)
   - Before delete: `git diff HEAD` in worktree → save to `~/.cwt/snapshots/<n>-<ts>.patch`
   - Also capture `git log --oneline <base>..HEAD` for any commits made
   - Save metadata alongside the patch

### Acceptance Criteria
- `cwt create test-feature --base main` creates a worktree at `.claude/worktrees/test-feature`
- `cwt list` shows all worktrees with their status
- `cwt delete test-feature` saves a snapshot then removes the worktree
- All operations work correctly on a real git repo

---

## Phase 2 — TUI Shell + Worktree List Panel

**Goal**: Interactive TUI launches, shows worktree list, supports navigation and basic actions.

### Tasks

1. **App state** (`src/app.rs`)
   - `App` struct holding: worktree list, selected index, active panel, dialog state, config
   - `AppMode` enum: Normal, Dialog(DialogKind), Help
   - Message/update pattern: input events → messages → state updates → render
2. **Event loop** (`src/main.rs` tui subcommand)
   - crossterm raw mode + alternate screen
   - tokio select between crossterm events (key/mouse/resize) and background tasks
   - 1Hz tick for session status polling
3. **Layout** (`src/ui/layout.rs`)
   - Two vertical panels: worktree list (35%) + inspector (65%)
   - Bottom bar: keybinding hints (context-sensitive)
   - Top bar: project name + repo path
4. **Worktree list widget** (`src/ui/worktree_list.rs`)
   - Grouped: ephemeral section + permanent section
   - Each item: status icon (●/✓/⏸/⚠) + name + age/duration
   - Highlight selected item
   - `j/k` navigation, section headers are non-selectable
5. **Inspector panel** (`src/ui/inspector.rs`)
   - Shows for selected worktree: branch, base, age, disk size, session status
   - Changed files list (`git diff --stat`)
   - Last message preview (placeholder — real transcript parsing in Phase 3)
6. **Create dialog** (`src/ui/dialogs/create.rs`)
   - Step-through dialog: name → branch → carry changes → confirm
   - Branch step uses fuzzy filter (type to filter branch list)
   - Tab/Enter to advance steps, Esc to cancel
7. **Delete dialog** (`src/ui/dialogs/delete.rs`)
   - Show: what will be snapshotted, what will be lost
   - y/n confirmation
8. **Theme** (`src/ui/theme.rs`)
   - Color constants, border styles, status icon definitions
   - Keep it minimal: catppuccin-ish palette that works on dark terminals

### Acceptance Criteria
- `cwt tui` (or just `cwt` with no args) launches the TUI
- Can navigate worktree list with j/k
- Can create a worktree via `n` → dialog flow
- Can delete a worktree via `d` → confirmation
- Inspector shows real git diff stat for selected worktree
- `q` exits cleanly (restores terminal)

---

## Phase 3 — tmux Session Management + Transcripts

**Goal**: Launch Claude Code in tmux panes, track session status, show transcript previews.

### Tasks

1. **tmux module** (`src/tmux/pane.rs`)
   - `is_in_tmux()` → check `$TMUX` env var, error if not in tmux
   - `create_pane(name, cwd, command)` → `tmux split-window -h -P -F '#{pane_id}' -t <session> "cd <cwd> && <cmd>"`
   - `focus_pane(pane_id)` → `tmux select-pane -t <id>`
   - `kill_pane(pane_id)` → `tmux kill-pane -t <id>`
   - `list_panes()` → parse `tmux list-panes` for pane status (alive/dead, command)
   - `pane_is_alive(pane_id)` → check if pane still exists
2. **Session launcher** (`src/session/launcher.rs`)
   - `launch(worktree)` → create tmux pane running `claude` in the worktree dir
   - `resume(worktree)` → if session_id known, `claude --resume <id>` in the worktree
   - `focus(worktree)` → switch to the tmux pane
   - Store pane_id in worktree state after launch
3. **Session tracker** (`src/session/tracker.rs`)
   - Discover Claude Code project dir: hash the worktree absolute path → find in `~/.claude/projects/`
   - List session files (`.jsonl` transcripts) sorted by mtime
   - Determine session state: check if tmux pane is alive + what command is running
   - Poll on the 1Hz tick cycle
4. **Transcript reader** (`src/session/transcript.rs`)
   - Parse Claude Code `.jsonl` transcript format
   - Extract last N assistant messages (just the text content, strip tool calls)
   - Truncate to ~200 chars for the preview
   - Extract token usage / cost if present in the transcript metadata
5. **Session keybinding** (`s` key)
   - If no session: launch new one
   - If session running: focus the tmux pane
   - If session dead but session_id known: offer to resume
6. **Inspector updates**
   - Show session status line: "running 5m" / "idle" / "waiting for input" / "done"
   - Show last message preview
   - Show tmux pane reference

### Acceptance Criteria
- Pressing `s` on a worktree opens Claude Code in a tmux pane to the right
- Pressing `s` again on same worktree focuses that pane
- Inspector shows real "last message" from the Claude session transcript
- Session status updates within ~1 second of changes
- Exiting the TUI does NOT kill running tmux panes (sessions survive)

---

## Phase 4 — Handoff + Setup Scripts + GC

**Goal**: All v0.1 must-have features complete.

### Tasks

1. **Handoff module** (`src/worktree/handoff.rs`)
   - `worktree_to_local(worktree)`:
     - In worktree dir: `git diff HEAD` → patch content
     - In local dir: `git apply <patch>` (or `git apply --3way` for conflicts)
     - If worktree has commits: `git format-patch <base>..HEAD` → `git am` in local
     - Return list of applied files
   - `local_to_worktree(worktree)`:
     - In local dir: `git diff` + `git diff --cached` → patch
     - In worktree dir: `git apply <patch>`
     - Optionally: `git stash` in local after transfer
   - Gitignore check: scan for untracked files in source that are in `.gitignore`, warn user
2. **Handoff dialog** (`src/ui/dialogs/handoff.rs`)
   - Show direction picker: WT→Local or Local→WT
   - Show diff preview of what will be transferred
   - Show gitignore warnings if applicable
   - Confirm/cancel
3. **Setup scripts** (`src/worktree/setup.rs`)
   - After worktree creation, if `config.setup.script` is set:
     - Run the script in the worktree directory
     - Stream stdout/stderr to a log (show in inspector as "Setting up...")
     - Timeout after `config.setup.timeout_secs`
     - If script fails: warn but don't delete the worktree
   - Common setup scripts: `npm install`, `pip install -e .`, `cargo build`, etc.
4. **GC module** (extend `src/worktree/manager.rs`)
   - `gc_preview()` → return list of worktrees that would be pruned
   - `gc_execute(worktrees)` → snapshot + delete each
   - Exclusion rules: running session, uncommitted changes, unpushed commits
   - Sort by last activity, prune oldest first
5. **GC dialog** (`src/ui/dialogs/gc.rs`)
   - Show: N ephemeral worktrees, limit is M, will prune K
   - List what will be pruned with their last activity time
   - List what's protected and why
   - Confirm/cancel
6. **Restore from snapshot** (`r` key)
   - List snapshots from `~/.cwt/snapshots/`
   - Show patch metadata (name, date, base commit, file count)
   - Apply: create a new worktree from the base commit, then `git apply` the patch

### Acceptance Criteria
- `h` on a worktree shows handoff dialog, can transfer changes in both directions
- Creating a worktree with a setup script configured runs the script automatically
- `g` shows GC preview with correct exclusion logic
- `r` can restore a previously-deleted worktree from its snapshot
- Full end-to-end workflow: create → launch session → do work → handoff to local → delete

---

## Phase 5 — Polish + Hooks + Packaging (stretch)

**Goal**: Production-ready CLI with Claude Code hooks integration and Nix packaging.

### Tasks

1. **Claude Code hooks** (`src/hooks/claude.rs`)
   - Generate hook config snippet for `.claude/settings.json`:
     - `WorktreeCreate` → notify cwt state
     - `WorktreeRemove` → notify cwt state
     - `Stop` → update session status
     - `Notification` → update session status to "waiting"
   - `cwt hooks install` subcommand to add hooks to settings.json
   - Hooks write to a unix socket or temp file that the TUI polls
2. **Diff viewer** — scrollable inline diff in inspector (syntax-highlighted if feeling fancy)
3. **Filter/search** — `/` to fuzzy filter worktree list by name
4. **Help overlay** — `?` shows all keybindings
5. **Error handling polish** — user-friendly messages for: not in git repo, not in tmux, claude not found, etc.
6. **Nix flake** — `flake.nix` with `packages.default` for the binary, devShell with rust toolchain
7. **README.md** with GIF/screenshot of the TUI in action
8. **`cwt` as default command** — no subcommand = launch TUI (same as `cwt tui`)
9. **Mouse support** — click to select worktree, click action buttons

---

## File-by-File Implementation Order

For Claude Code sessions, tackle files in this order within each phase:

### Phase 1
```
1. Cargo.toml
2. src/main.rs (clap CLI skeleton)
3. src/git/mod.rs + commands.rs (worktree list/add/remove)
4. src/git/branch.rs (list branches, current branch)
5. src/git/diff.rs (diff stat, full diff)
6. src/worktree/model.rs (structs + enums)
7. src/config/model.rs (config structs + defaults)
8. src/config/mod.rs (loader)
9. src/state/store.rs (JSON persistence)
10. src/state/mod.rs
11. src/worktree/snapshot.rs
12. src/worktree/manager.rs (ties everything together)
13. src/worktree/mod.rs
```

### Phase 2
```
1. src/ui/theme.rs (colors, symbols)
2. src/ui/layout.rs (panel structure)
3. src/ui/worktree_list.rs (list widget)
4. src/ui/inspector.rs (detail panel)
5. src/ui/dialogs/create.rs
6. src/ui/dialogs/delete.rs
7. src/ui/dialogs/mod.rs
8. src/ui/mod.rs
9. src/app.rs (state + update loop)
10. update src/main.rs (tui subcommand + event loop)
```

### Phase 3
```
1. src/tmux/pane.rs
2. src/tmux/mod.rs
3. src/session/launcher.rs
4. src/session/tracker.rs
5. src/session/transcript.rs
6. src/session/mod.rs
7. update src/ui/inspector.rs (session info)
8. update src/app.rs (session keybinding, polling)
```

### Phase 4
```
1. src/worktree/handoff.rs
2. src/worktree/setup.rs
3. src/ui/dialogs/handoff.rs
4. src/ui/dialogs/gc.rs
5. update src/worktree/manager.rs (gc logic)
6. update src/app.rs (handoff + gc + restore keybindings)
```
