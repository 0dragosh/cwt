# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.14](https://github.com/0dragosh/cwt/compare/v0.2.13...v0.2.14) - 2026-04-23

### Other

- fix flake build

## [0.2.13](https://github.com/0dragosh/cwt/compare/v0.2.12...v0.2.13) - 2026-04-23

### Added

- add Verus workflow verification sidecar ([#51](https://github.com/0dragosh/cwt/pull/51))

## [0.2.12](https://github.com/0dragosh/cwt/compare/v0.2.11...v0.2.12) - 2026-04-23

### Added

- add Pi session provider ([#49](https://github.com/0dragosh/cwt/pull/49))

## [0.2.11](https://github.com/0dragosh/cwt/compare/v0.2.10...v0.2.11) - 2026-04-16

### Other

- *(deps)* bump rand from 0.10.0 to 0.10.1 ([#44](https://github.com/0dragosh/cwt/pull/44))

## [0.2.10](https://github.com/0dragosh/cwt/compare/v0.2.9...v0.2.10) - 2026-04-16

### Fixed

- use dedicated token for release PR CI
- dispatch CI for release PRs
- build linux release assets on ubuntu 22.04 ([#46](https://github.com/0dragosh/cwt/pull/46))

## [0.2.9](https://github.com/0dragosh/cwt/compare/v0.2.8...v0.2.9) - 2026-03-18

### Other

- Auto-close panes when cwt and sessions exit ([#40](https://github.com/0dragosh/cwt/pull/40))
- zellij

## [0.2.8](https://github.com/0dragosh/cwt/compare/v0.2.7...v0.2.8) - 2026-03-18

### Added

- Add zellij-backed multiplexer support with tmux fallback ([#38](https://github.com/0dragosh/cwt/pull/38))
- add cwt prompt output for starship worktree context ([#36](https://github.com/0dragosh/cwt/pull/36))

### Other

- *(release)* drop windows release artifacts ([#34](https://github.com/0dragosh/cwt/pull/34))

## [0.2.7](https://github.com/0dragosh/cwt/compare/v0.2.6...v0.2.7) - 2026-03-18

### Other

- Update macOS release runner to macos-15-intel ([#32](https://github.com/0dragosh/cwt/pull/32))

## [0.2.6](https://github.com/0dragosh/cwt/compare/v0.2.5...v0.2.6) - 2026-03-18

### Fixed

- binary publishing

## [0.2.5](https://github.com/0dragosh/cwt/compare/v0.2.4...v0.2.5) - 2026-03-18

### Other

- Add cargo-binstall metadata and release binary uploads ([#29](https://github.com/0dragosh/cwt/pull/29))

## [0.2.4](https://github.com/0dragosh/cwt/compare/v0.2.3...v0.2.4) - 2026-03-18

### Fixed

- provider not working (codex) ([#25](https://github.com/0dragosh/cwt/pull/25))

### Other

- switch terminology to provider and document runtime provider toggle ([#23](https://github.com/0dragosh/cwt/pull/23))

## [0.2.3](https://github.com/0dragosh/cwt/compare/v0.2.2...v0.2.3) - 2026-03-15

### Added

- Add provider abstraction and OpenAI Codex CLI support ([#21](https://github.com/0dragosh/cwt/pull/21))

## [0.2.2](https://github.com/0dragosh/cwt/compare/v0.2.1...v0.2.2) - 2026-03-13

### Other

- Use task title and description for PR creation instead of worktree name ([#18](https://github.com/0dragosh/cwt/pull/18))

## [0.2.1](https://github.com/0dragosh/cwt/compare/v0.2.0...v0.2.1) - 2026-03-13

### Added

- provider custom cmds ([#16](https://github.com/0dragosh/cwt/pull/16))

## [0.2.0](https://github.com/0dragosh/cwt/compare/v0.1.4...v0.2.0) - 2026-03-13

### Added

- [**breaking**] Add comprehensive code review: 47 issues across all modules ([#14](https://github.com/0dragosh/cwt/pull/14))
- automerge release-plz PRs in workflow
- automerge release-plz PRs

### Fixed

- allow release-plz to push PR branches
