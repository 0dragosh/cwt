# Verification Guide for Agents

This document explains why `cwt` has a Verus sidecar verification layer, what
it is useful for, and what an agent should update when changing workflow
behavior in this repository.

## Purpose

The verification layer exists to keep a small set of workflow guarantees
explicit and hard to regress.

`cwt` manages mutable state across git worktrees, snapshots, session state, and
handoff operations. Those flows are easy to break with local changes that look
reasonable in isolation. The sidecar model gives agents a place to state the
intended invariants directly, while Rust tests check that the real code still
behaves consistently with those invariants.

This is intentionally not a proof of all production code. It is a proof of the
workflow rules we care about most.

## Why This Is Useful

For agents modifying this repo, the sidecar helps in three ways:

1. It makes the intended behavior of create, delete, promote, GC, restore, and
   handoff concrete instead of implicit.
2. It forces trusted boundaries to be named, rather than hand-waved.
3. It gives a second line of defense beyond ordinary Rust tests: an abstract
   model plus conformance tests against real temporary repos.

In practice, this is most useful when you are:

- changing worktree lifecycle rules
- changing what GC is allowed to prune
- changing snapshot / restore behavior
- changing handoff semantics
- adding a new workflow transition that should preserve existing invariants

## Verus Principles We Rely On

The sidecar follows a few core principles from the Verus guide:

- Verus is for static verification. The goal is to prove properties ahead of
  time rather than adding runtime checks to production code.
- Verus separates `spec`, `proof`, and `exec` concerns. In `cwt`, we keep the
  verified model in sidecar `spec` / `proof` code and leave production Rust as
  ordinary executable code.
- Verification always depends on a trusted computing base. If some behavior is
  outside the proof boundary, it must be called out explicitly as an assumption.

Useful references:

- <https://verus-lang.github.io/verus/guide/>
- <https://verus-lang.github.io/verus/guide/modes.html>
- <https://verus-lang.github.io/verus/guide/tcb.html>

## Why A Sidecar Instead Of Annotating `src/`

For this repo, the sidecar approach is deliberate.

We want the first pass to verify workflow correctness without refactoring the
production code into verifier-friendly shapes. `cwt` depends on git commands,
filesystem effects, tmux state, and CLI behavior. Treating those as fully
verified implementation details would expand scope sharply and slow iteration.

The sidecar model keeps the proof target narrow:

- model the workflow state transitions abstractly
- prove invariants over those transitions
- validate the real implementation with Rust tests on real git repos

That gives useful assurance without pretending we have verified git, tmux, or
the operating system.

## What Is Verified

The current sidecar model in [verification/workflow.rs](../verification/workflow.rs)
covers:

- create adds a fresh idle ephemeral worktree
- delete removes exactly one worktree after snapshot success
- promote is idempotent and never demotes permanent worktrees
- GC preview only selects safe ephemeral worktrees and never exceeds the
  required excess
- restore recreates a worktree from snapshot metadata without consuming the
  snapshot
- handoff preserves source changes and only mutates the intended target on
  successful apply

The matching conformance coverage lives in
[tests/workflow_conformance.rs](../tests/workflow_conformance.rs), with a few
targeted production-path tests in
[src/worktree/manager.rs](../src/worktree/manager.rs) and
[src/worktree/handoff.rs](../src/worktree/handoff.rs).

## What Is Not Verified

The sidecar does not prove:

- git itself
- patch application internals
- tmux or zellij behavior
- filesystem atomicity beyond the assumptions we document
- wall-clock correctness
- TUI rendering or interaction logic
- the Verus toolchain, Rust compiler, or solver

Those are trusted boundaries and must stay documented in
[verification/ASSUMPTIONS.md](../verification/ASSUMPTIONS.md).

## Files You Must Update

When you change workflow behavior, check these files deliberately:

### `verification/workflow.rs`

Update this when the abstract workflow rules change.

Examples:

- a new lifecycle state changes whether something is GC-safe
- restore semantics change
- handoff changes what counts as source preservation
- a new transition should preserve existing invariants

If production behavior changes and this file does not, that is usually a smell.

### `verification/ASSUMPTIONS.md`

Update this when the trust boundary changes.

Examples:

- new dependence on external command truth
- new filesystem assumption
- new runtime fact relied on by the workflow model

If you rely on something outside the proof boundary, write it down here.

### `tests/workflow_conformance.rs`

Update this when the real implementation should be checked against a changed or
new invariant.

The rule is simple: if the Verus model says some behavior matters, there should
usually be a Rust test that exercises the real code path too.

### `scripts/verify-verus.sh`

Update this if the verification entrypoint changes.

Right now it is intentionally simple: run all Verus files under
`verification/`, and fail early with the exact `rustup install` command when
the required toolchain is missing.

## How To Extend The Verification Layer

When adding a new workflow rule:

1. Decide whether it belongs in the abstract model or only in production tests.
2. If it is a workflow invariant, encode it in `verification/workflow.rs`.
3. Document any new trust boundary in `verification/ASSUMPTIONS.md`.
4. Add or extend Rust conformance tests.
5. Keep the model narrow. Do not pull unrelated implementation detail into the
   proof just because it exists.

Good candidates:

- selection rules
- preservation properties
- idempotence properties
- “only mutates X” style guarantees
- “never chooses Y” style safety rules

Bad candidates for this layer:

- UI details
- subprocess output formatting
- low-level git behavior that is better treated as trusted command truth

## How To Run It

For normal repo validation:

```sh
cargo test
nix build
```

For the Verus environment:

```sh
nix develop .#verus
./scripts/verify-verus.sh
```

Or in one command:

```sh
nix develop .#verus -c ./scripts/verify-verus.sh
```

The pinned Verus release currently requires an exact Rust toolchain via
`rustup`. The script does not install that toolchain for you; it prints the
exact `rustup install ...` command instead.

## How To Read Failures

If a Verus run fails:

- first ask whether the workflow model is stale
- then ask whether the production behavior is wrong
- then ask whether a new assumption has been introduced but not documented

If Rust conformance tests fail while Verus still passes:

- the abstract model is likely still true, but the real implementation drifted
- fix the implementation, or change the model and tests together if the behavior
  change is intended

If production code changes but neither the model nor the tests need updates,
that is fine only if the change is outside the modeled workflow surface.

## Agent Checklist

Before finishing a workflow-related change, check:

- Did I change a modeled transition?
- Did I change a trusted boundary?
- Did I update the conformance tests?
- Did I keep the proof target narrow?
- Did I rerun the relevant Rust and Verus commands?

If the answer to the first two questions is yes and the docs/model stayed
unchanged, revisit the change before you call it done.

