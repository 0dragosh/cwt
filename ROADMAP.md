# cwt Roadmap

Beyond the v0.1 build plan (Phases 1-4), here's the full evolution.

## v0.1 — Core (Phases 1-4)

Worktree CRUD, TUI, tmux sessions, handoff, setup scripts, GC, snapshots.
See PLAN.md for details.

## v0.2 — Live State (Phase 5)

**Theme**: cwt becomes reactive instead of poll-based.

### Claude Code Hooks Integration
- `cwt hooks install` subcommand — patches `.claude/settings.json` with hook entries
- Hooks: WorktreeCreate, WorktreeRemove, Stop, Notification, SubagentStop
- Each hook writes a JSON event to a Unix domain socket: `/tmp/cwt-<repo-hash>.sock`
- TUI reads from the socket on its event loop — sub-second status updates
- Works bidirectionally: worktrees created via `claude --worktree` outside cwt appear instantly

### Hook Event Schema
```json
{
  "event": "session_stop",
  "worktree": "feature-auth",
  "session_id": "abc123",
  "timestamp": "2026-03-11T15:30:00Z",
  "data": {
    "exit_reason": "complete"
  }
}
```

### Hook Scripts
- Tiny shell scripts that `cwt hooks install` writes to `.cwt/hooks/`
- Each reads Claude Code's JSON stdin, transforms it, writes to the socket
- Zero dependencies beyond bash + jq (or inline JSON with printf)

### Session Status State Machine
```
Created → Running → Done
                 ↘ Waiting (needs input)
                 ↘ Error (session crashed)
```

### Other Polish
- `/` fuzzy filter in worktree list
- `?` help overlay with all keybindings
- Mouse click support for worktree selection
- Scrollable diff viewer in inspector
- Graceful error handling: not in git repo, not in tmux, claude not found, git conflicts during handoff
- `cwt` with no args = launch TUI (drop the `tui` subcommand requirement)

---

## v0.3 — Forest Mode (Phase 6)

**Theme**: manage worktrees across multiple repos from one TUI.

### Multi-Repo Support
- New top-level view: repo picker (list of registered git repos)
- `cwt add-repo <path>` to register a repo
- `cwt forest` to launch in forest mode
- Config at `~/.config/cwt/forest.toml`:
  ```toml
  [[repo]]
  path = "~/code/gideon"
  name = "gideon"

  [[repo]]
  path = "~/code/homelab"
  name = "homelab"
  ```

### TUI Changes
- Three-panel layout in forest mode: repos | worktrees | inspector
- `R` to switch repos, or click in repo panel
- Global dashboard: "4 sessions running across 3 repos"
- Global GC: prune across all repos

### Cross-Repo State
- Each repo keeps its own `.cwt/state.json`
- Global index at `~/.config/cwt/index.json` tracks all repos + aggregate stats
- `cwt status` CLI command: one-line summary of all repos and active sessions

---

## v0.4 — Agent Orchestration (Phase 7)

**Theme**: cwt becomes a parallel task dispatcher, not just a worktree manager.

### Task-Based Creation
- `cwt dispatch "implement auth" "add tests" "update docs"` — creates N worktrees, one per task
- Each worktree launches a Claude Code session with the task as the initial prompt
- Interactive version: `t` in TUI → enter tasks line by line → confirm → dispatch all

### Issue Import
- `cwt import --github` — fetch issues from GitHub, create worktrees per issue
- `cwt import --linear` — same for Linear
- PR auto-linking: commit messages include `Fixes #N`

### Broadcast
- `b` in TUI → type a prompt → send to all running sessions
- Use Claude Code's stdin pipe or `--prompt` flag
- Useful for: "make sure all modules handle the new error type"

### Dependency Graph (stretch)
- Define task ordering: "tests should start after implementation finishes"
- Visual DAG in the TUI
- Auto-launch dependent tasks when predecessors complete (via Stop hook)

### Aggregate Dashboard
- Progress bars per session (based on transcript analysis)
- Token usage / cost totals across all active sessions
- ETA estimates based on message rate

---

## v0.5 — Ship Pipeline (Phase 8)

**Theme**: close the loop from worktree to merged PR.

### PR Integration
- `P` on a worktree → commit staged changes → push → create PR via `gh`
- PR template: auto-filled with session transcript summary (ask Claude to summarize)
- PR status in worktree list: 🟡 draft | 🔵 review | 🟢 approved | 🟣 merged
- After merge: auto-cleanup worktree + branch

### Ship Flow
- `S` (shift-s) → "ship it" macro:
  1. Handoff worktree → local (or just push from worktree branch)
  2. Create PR
  3. Mark worktree as "shipping"
  4. On merge (polled or webhook): delete worktree
- One-keypress from "done" to "PR open"

### CI Status
- Poll GitHub Actions status for worktree branches
- Show ✅/❌/⏳ next to PR status
- `c` to open CI logs in browser

---

## v0.6 — Environments (Phase 9)

**Theme**: each worktree gets isolated runtime, not just isolated files.

### Per-Worktree Containers
- Setup script can be a `Containerfile` / `devcontainer.json`
- `cwt` manages container lifecycle alongside tmux panes
- Container runs with worktree mounted as a volume
- Claude Code session runs inside the container

### Port Management
- Auto-assign non-conflicting ports per worktree
- Port map visible in inspector: "localhost:3001 → feature-auth, localhost:3002 → bugfix-123"
- Env vars injected: `CWT_PORT=3001`, `CWT_DB_PORT=5433`, etc.

### Resource Tracking
- Disk usage per worktree (including node_modules, venv, build artifacts)
- Container resource usage (CPU, memory)
- Warnings when approaching disk/resource limits

### Implementation Notes
- Use Podman (rootless) over Docker for NixOS compatibility
- Quadlet-style management if on NixOS
- Fallback to bare setup scripts on systems without container runtime

---

## v0.7 — Remote (Phase 10)

**Theme**: run worktrees on remote machines.

### Remote Hosts
- Config: register remote hosts with SSH connection details
  ```toml
  [[remote]]
  name = "fenrir"
  host = "fenrir.local"
  user = "d"
  worktree_dir = "/data/worktrees"
  ```
- `cwt create --remote fenrir feature-auth` → creates worktree on remote machine
- Session runs in remote tmux via SSH

### Sync
- State sync: local cwt knows about remote worktrees
- Handoff across machines: generate patch locally, apply remotely (or vice versa)
- Git push/pull as the sync mechanism for code changes

### TUI Indicators
- Remote worktrees show host name: `feature-auth [fenrir]`
- Network status indicator
- Latency-aware: batch status updates, don't poll remote on every tick

---

## Architecture Evolution

### v0.1-0.2: Single binary, single repo
```
cwt (TUI) → git CLI → local repo
           → tmux → claude sessions
```

### v0.3: Multi-repo aware
```
cwt (TUI) → git CLI → repo A, repo B, repo C
           → tmux → claude sessions (per repo)
           → state index across repos
```

### v0.4-0.5: Orchestrator
```
cwt (TUI) → git CLI → repos
           → tmux → claude sessions
           → gh CLI → GitHub PRs + issues
           → task graph → dependency resolution
```

### v0.6-0.7: Distributed
```
cwt (TUI) → git CLI → local repos
           → tmux → local sessions
           → podman → containers per worktree
           → SSH → remote hosts
             → git → remote repos
             → tmux → remote sessions
```

---

## Non-Goals (things cwt will never be)

- **An IDE**: cwt manages worktrees and sessions. Editing happens in your editor.
- **A git GUI**: use lazygit/gitui for complex git operations. cwt only does worktree-specific git ops.
- **A Claude Code wrapper**: cwt doesn't intercept or modify Claude Code behavior. It orchestrates around it.
- **A CI/CD tool**: cwt can trigger PR creation but doesn't manage pipelines.
