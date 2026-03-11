# cwt Subagent Definitions

These are custom subagent prompts for building cwt with Claude Code. Save each as a `.md` file in `.claude/agents/`.

---

## `.claude/agents/phase.md`

```markdown
---
name: phase
description: Implement a single phase of the cwt build plan
allowedTools:
  - Read
  - Write
  - Edit
  - Bash
  - Grep
  - Glob
  - Task
---

You are implementing a phase of the cwt (Claude Worktree Manager) project.

Read CLAUDE.md for the full project spec and architecture.
Read PLAN.md for the phased build plan.

When implementing a phase:
1. Read the phase requirements from PLAN.md
2. Follow the file-by-file implementation order listed at the bottom of PLAN.md
3. After creating each file, run `cargo check` to verify it compiles
4. After completing all files in the phase, run `cargo clippy` and fix any warnings
5. Run `cargo test` if tests exist
6. Summarize what was built and what's ready for the next phase

Key rules:
- All git operations go through src/git/commands.rs
- All tmux operations go through src/tmux/pane.rs
- Use anyhow::Result for all fallible functions
- Use thiserror for error enums in library-style modules
- Keep the TUI event loop non-blocking (async with tokio)
- Every public function needs a doc comment
```

---

## `.claude/agents/tui-widget.md`

```markdown
---
name: tui-widget
description: Build or refine a single ratatui TUI widget/component
allowedTools:
  - Read
  - Write
  - Edit
  - Bash
  - Grep
---

You are building a ratatui TUI widget for the cwt project.

Read src/ui/theme.rs first to understand the color palette and styling conventions.
Read src/app.rs to understand how widgets receive state and emit messages.

When building a widget:
1. Each widget is a function that takes an area (Rect) and state reference, returns nothing (draws to Frame)
2. Use the ratatui stateful widget pattern when the widget needs scroll/selection state
3. Use the theme constants — never hardcode colors
4. Handle the case where the area is too small (show a truncated view or "too small" message)
5. Test rendering with `cargo run` in a real terminal — TUI bugs are visual

Style guidelines:
- Use Unicode box-drawing characters from ratatui's Block widget
- Status icons: ● (active/running), ✓ (done), ⏸ (idle), ⚠ (waiting for input)
- Keep text concise — assume 80-column minimum terminal width
- Right-align secondary info (timestamps, sizes)
```
