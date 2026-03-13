# Contributing to cwt

Thanks for your interest in contributing to cwt! This guide will help you get started.

## Development Setup

### Prerequisites

- **Rust** (stable, 2021 edition)
- **git** (with worktree support)
- **tmux**

### Using Nix (recommended)

The project includes a Nix flake that provides a complete development environment:

```sh
nix develop
```

This gives you Rust toolchain, `rust-analyzer`, `cargo-watch`, `cargo-edit`, plus `git` and `tmux`.

### Manual Setup

```sh
git clone https://github.com/0dragosh/cwt.git
cd cwt
cargo build
cargo test
```

## Making Changes

1. **Fork and clone** the repository
2. **Create a branch** from `main` for your changes
3. **Make your changes** -- keep commits focused and atomic
4. **Run checks** before submitting:

```sh
cargo clippy            # Lint -- must pass with zero warnings
cargo test              # Run the test suite
cargo fmt --check       # Check formatting
```

5. **Open a pull request** against `main`

## Code Conventions

- Use `anyhow::Result` for application-level errors
- Use `thiserror` for error enums in library-style modules (git, tmux, config, state)
- All git operations go through `src/git/commands.rs` -- never shell out to git from other modules
- All tmux operations go through `src/tmux/pane.rs`
- The TUI event loop is async (tokio) -- keep it non-blocking
- Follow existing code style; `cargo fmt` is enforced in CI

## Project Structure

```
src/
  main.rs          # CLI parsing, TUI bootstrap
  app.rs           # App state, event loop, rendering
  config/          # TOML config loading
  state/           # JSON state persistence
  git/             # Git worktree, branch, diff ops
  worktree/        # Worktree CRUD, handoff, snapshots
  session/         # Claude session launcher + tracker
  tmux/            # tmux pane management
  hooks/           # Unix socket listener, hook scripts
  forest/          # Multi-repo config + index
  orchestration/   # Task dispatch, issue import, broadcast
  ship/            # PR creation, CI status
  env/             # Container support, ports, resources
  remote/          # SSH remote host management
  ui/              # ratatui widgets and dialogs
```

## Testing

Integration tests live in `tests/integration.rs` and create temporary git repos for isolation. Run them with:

```sh
cargo test
```

When adding new features, add corresponding integration tests. Tests should:
- Create a temp directory with `tempfile::tempdir()`
- Initialize a git repo in it
- Exercise the feature via CLI commands or library calls
- Assert on state files, git state, or command output

## Reporting Issues

- Use [GitHub Issues](https://github.com/0dragosh/cwt/issues)
- Include your OS, Rust version, and `cwt --version` output
- For bugs: steps to reproduce, expected vs actual behavior
- For features: describe the use case and how you'd expect it to work

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
