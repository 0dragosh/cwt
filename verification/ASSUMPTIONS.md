# Verus Sidecar Assumptions

This verification suite models cwt workflow state transitions. It does not
prove git, tmux, filesystem, terminal UI, or operating system behavior.

Trusted boundaries:

- git command truth: git command wrappers report the actual repository,
  worktree, branch, dirty, and upstream state.
- patch application semantics: `git apply --3way` and `git am --3way` apply
  exactly the patch or mailbox content supplied to the target repository.
- filesystem atomicity: snapshot temp-file writes followed by rename either
  leave a complete snapshot entry or surface an error before state deletion.
- tmux/session facts: session status recorded in cwt state correctly reflects
  whether a worktree is running and should be protected from GC.
- wall-clock timestamps: creation and deletion timestamps are trusted for
  ordering, auditing, and display, but not for safety-critical proof steps.
- Verus/Rust compiler trust: the Verus verifier, Rust compiler, standard
  libraries, and platform toolchain are part of the trusted computing base.

The sidecar model verifies workflow invariants over abstract state:

- create adds only a fresh idle ephemeral worktree;
- delete removes exactly one worktree after snapshot success;
- promote is idempotent and never demotes permanent worktrees;
- GC preview selects only safe ephemeral worktrees and never exceeds the
  requested excess;
- restore recreates a worktree from snapshot metadata without consuming the
  snapshot;
- handoff preserves source changes and only mutates the intended target on
  successful apply.

