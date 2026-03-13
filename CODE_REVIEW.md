# CWT Deep Code Review — Bug & Issue Report

**Date**: 2026-03-13
**Reviewer**: Senior Rust Engineer (automated deep review)
**Scope**: Full codebase — all modules in `src/`, `tests/`, `Cargo.toml`

---

## CRITICAL (9 issues)

### 1. Data Loss: Stash Not Restored on Worktree Creation Failure
**`src/worktree/manager.rs:82-99`** — If `git worktree add` fails *after* stashing the user's uncommitted changes, the `?` propagates the error without restoring the stash. The user's work is silently lost.

### 2. Command Injection via Setup Script Path
**`src/worktree/setup.rs:39-40`** — The setup script is passed to `sh -c` without shell escaping. A config value like `setup.script = "foo; rm -rf /"` executes arbitrary commands.

### 3. Command Injection in Remote SSH Commands
**`src/remote/host.rs:156-266`** — Multiple functions build SSH shell commands via `format!()` with unescaped user inputs (`worktree_name`, `branch_name`, `repo_url`, `base_branch`). Single-quoting is used but inner single quotes are not escaped, allowing shell breakout.

### 4. Command Injection in Remote Session Launch
**`src/remote/session.rs:29-31, 56-67`** — `claude_args` from user config are joined and interpolated into a tmux command without escaping. A malicious arg like `'; rm -rf /; echo '` executes arbitrary commands on the remote host.

### 5. Prompt Injection via Broadcast / tmux send-keys
**`src/orchestration/broadcast.rs:31`** — User-supplied prompt is sent directly to `tmux send-keys` without sanitization. Control sequences (`C-c`, `C-u`) or shell metacharacters can interrupt sessions or execute commands if the pane is in shell mode.

### 6. Command Injection in Task Dispatch
**`src/orchestration/dispatch.rs:79-109`** — `shell_quote()` only escapes single quotes but not backticks, `$()`, or newlines. The quoted prompt is joined into a compound shell command and passed to `tmux::pane::create_pane`.

### 7. Path Traversal in Snapshot Save
**`src/worktree/snapshot.rs:10-12, 56`** — A worktree named `../../etc/cron.d/evil` results in `~/.cwt/snapshots/../../etc/cron.d/evil-<ts>.patch`, writing outside the intended directory.

### 8. UTF-8 Panic in Message Truncation
**`src/session/transcript.rs:203-210`** — `truncate_message()` slices with `msg[..max_chars]` using byte indices, not character indices. Any multi-byte character (emoji, CJK, accented) at the boundary triggers `panic!("byte index N is not a char boundary")`.

### 9. UTF-8 Panic in Worktree Filter UI
**`src/ui/worktree_list.rs:63-65`** — `pos` is found via lowercased string but used to slice the original `wt.name`. Multi-byte characters cause a panic on char boundary mismatch.

---

## HIGH (14 issues)

### 10. Race Condition: State File Read-Modify-Write Without Locking
**`src/app.rs:168-179, 269-275`** — Multiple async paths (refresh timer, hook events, user actions) all do `load → modify → save` on `state.json` without mutual exclusion. Concurrent writes silently overwrite each other.

### 11. GC Off-by-One: Skipped Worktrees Not Replaced
**`src/worktree/manager.rs:220`** — The GC iterates over the first `to_prune` ephemerals but skips those with running sessions or dirty state. Skipped entries are not replaced, so GC never reaches `max_ephemeral`.

### 12. Incomplete Handoff: Commits Applied But Uncommitted Patch Fails
**`src/worktree/handoff.rs:156-194`** — If `apply_mailbox` succeeds but the subsequent uncommitted-changes patch fails, commits are already in local but the error is returned. The repo is left in a partial handoff state with no rollback.

### 13. Stash Detection Logic Flaw
**`src/git/commands.rs:179-184`** — `stash()` compares `git stash list` before/after to detect success. If another process creates a stash concurrently, the function falsely reports success even if `git stash push` failed.

### 14. TOCTOU Race in Worktree Deletion
**`src/worktree/manager.rs:146-170`** — `wt_abs_path.exists()` is checked, then `remove_dir_all()` is called. Between the check and removal, the directory could change. Symlinks inside could cause deletion of unintended paths.

### 15. TOCTOU in Session Status Check
**`src/session/tracker.rs:7-30`** — Two separate tmux queries (`pane_exists` then `pane_current_command`) are not atomic. The pane can be destroyed or recycled between calls, returning incorrect status.

### 16. Port Allocation Race Condition
**`src/env/ports.rs:168-190`** — `is_port_free()` binds and releases a port, but another process can claim it before the caller uses it. Classic TOCTOU.

### 17. Unbounded Channel Growth in Hook Socket
**`src/hooks/socket.rs:40`** — `mpsc::channel()` is unbounded. A flood of hook events (or bug in Claude Code) grows the channel without limit, eventually causing OOM.

### 18. Blocking Socket Accept Prevents Clean Shutdown
**`src/hooks/socket.rs:88-91`** — `set_nonblocking(false)` is called (blocking mode), but the code has a `WouldBlock` handler that can never trigger. The listener thread blocks forever on `accept()`, preventing clean TUI exit.

### 19. State Reconciliation Not Persisted
**`src/worktree/manager.rs:31-34`** — `load_state()` reconciles in-memory state with git but doesn't save. If multiple instances run, each gets its own reconciliation that's never persisted.

### 20. Credential Exposure: Linear API Key as CLI Argument
**`src/orchestration/import.rs:113-125`** — `LINEAR_API_KEY` is passed as a `-H` flag to `curl`, making it visible in `ps aux` and system logs.

### 21. PR Number Extraction Fragility
**`src/ship/pr.rs:248-253`** — `rsplit('/').next()` on the PR URL doesn't handle fragments or query params (e.g., `pull/42#discussion`), silently defaulting to PR number `0`.

### 22. Unwrap Panic in Import Dispatch Result
**`src/orchestration/import.rs:202`** — `result.into_iter().next().unwrap()` panics if `dispatch_tasks()` returns an empty vec.

### 23. Container Memory Parsing Out-of-Bounds
**`src/env/container.rs:344-348`** — `parts[1]` is accessed without bounds check. Malformed container stats output causes an index-out-of-bounds panic.

---

## MEDIUM (15 issues)

### 24. Corrupted state.json Locks Out TUI
**`src/state/store.rs:57-58`** — No recovery path if `state.json` is corrupted (e.g., partial write from crash). The TUI refuses to start.

### 25. Non-Atomic Snapshot Write
**`src/worktree/snapshot.rs:56-64`** — `fs::write()` is not atomic. Disk-full or crash during write leaves a truncated `.patch` file. Should write to temp file then rename.

### 26. Setup Script Path Traversal
**`src/worktree/setup.rs:32-37`** — Relative `script` path joined with worktree path isn't validated. `../../../etc/passwd` resolves outside the worktree.

### 27. Orphan Cleanup Silently Ignores Errors
**`src/worktree/manager.rs:388-392`** — `let _ = git::commands::worktree_prune(...)` swallows errors, potentially leaving git's internal state inconsistent.

### 28. Silent Diff Fallback Hides Real Errors
**`src/git/diff.rs:15-35`** — If `git diff --stat HEAD` fails due to permissions or corruption, the code silently retries without `HEAD` and returns potentially wrong results.

### 29. Selection State Drift After Hook Events
**`src/app.rs:208-266`** — `refresh()` reloads and re-sorts the worktree list. The selected index is clamped but may now point to a different worktree than the user intended.

### 30. Unbounded Transcript File Load
**`src/session/transcript.rs:35-36`** — `read_to_string` loads entire JSONL transcript into memory. 100MB+ transcripts from long sessions cause multi-second UI freezes and OOM risk.

### 31. Inspector Metadata Overflow
**`src/ui/inspector.rs:67-99`** — `meta_height` grows with conditional fields but is never clamped to `inner.height`, potentially hiding the diff section entirely.

### 32. CI Status Stale Run Detection
**`src/ship/ci.rs:39-50`** — Latest CI run is checked without verifying it belongs to the current HEAD commit. Shows stale pass/fail status.

### 33. Container Name Collision
**`src/env/container.rs:391-392`** — Container names are derived from worktree names without uniqueness guarantees. Two repos with same worktree name collide.

### 34. Socket Cleanup on Crash
**`src/hooks/socket.rs:34-37, 72-76`** — `Drop` handler removes socket file, but doesn't run on force-kill. Stale sockets accumulate in `/tmp`.

### 35. Dialog State Not Cleared on Error
**`src/app.rs:750-752`** — When worktree creation fails, the dialog may remain in an active state, causing confusing behavior on the next keypress.

### 36. Port Manager State Leak
**`src/app.rs:83-93, 2000`** — Externally deleted worktrees (not through TUI) leave stale port allocations that are never released.

### 37. UTF-8 Commit Hash Slicing
**`src/ui/inspector.rs:130`, `src/main.rs:415`** — `&base_commit[..8]` assumes ASCII. Corrupted state data could panic.

### 38. PR JSON Response Not Validated
**`src/ship/pr.rs:261-316`** — GitHub API responses are parsed with `.get()` chains defaulting to `None`/`0` on any shape mismatch. No distinction between API error and unexpected format.

---

## LOW (9 issues)

### 39. Config Not Hot-Reloaded
**`src/config/mod.rs:9-35`** — Config is loaded once at startup. Runtime changes require TUI restart.

### 40. Unused `build_shell_cmd()` Function
**`src/tmux/pane.rs:164-166`** — Dead code; the inline equivalent is used at line 32.

### 41. Hardcoded Dialog Heights
**`src/ui/dialogs/*.rs`** — Dialogs assume 14-18 line minimum height. Small terminals get truncated dialogs.

### 42. `pane_exists()` Hides tmux Failures
**`src/tmux/pane.rs:66-73`** — Returns `false` on any error (including tmux not running), masking real issues.

### 43. Tick Counter Wrapping Semantics
**`src/main.rs:291`** — `wrapping_add(1)` on `u32` is correct but relies on implicit modular arithmetic.

### 44. Layout Index Assumptions
**`src/ui/layout.rs`, `src/ui/dialogs/*.rs`** — `chunks[N]` accesses assume ratatui produces exactly as many rects as constraints. Extremely small terminals could violate this.

### 45. `from_utf8_lossy` Mangles Error Messages
**`src/git/commands.rs:22-24`** — Non-UTF8 git stderr is silently mangled, making debugging harder.

### 46. No MSRV Declared
**`Cargo.toml`** — No `rust-version` field. Implicit MSRV is unclear.

### 47. `tokio` Full Features
**`Cargo.toml`** — `features = ["full"]` pulls unnecessary sub-crates. Could be narrowed.

---

## Summary

| Severity | Count | Top Themes |
|----------|-------|------------|
| **CRITICAL** | 9 | Command injection (5), data loss (2), UTF-8 panics (2) |
| **HIGH** | 14 | Race conditions (5), state corruption (3), panics (3), credential exposure (1) |
| **MEDIUM** | 15 | Error handling (5), resource management (4), UI edge cases (3) |
| **LOW** | 9 | Dead code, config, dependency hygiene |

## Top 3 Action Items

1. **Shell escaping** — Every `format!()` building a shell command needs proper escaping. Consider a `shell_escape()` utility or use `Command::arg()` instead of string concatenation.
2. **Stash restore on failure** — Wrap the worktree creation in a guard that restores the stash on any error path.
3. **UTF-8 safe string operations** — Replace all `&s[..N]` byte slicing with `s.chars().take(N).collect::<String>()` or a `char_boundary`-aware helper.
