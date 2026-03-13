# Nix Integration

cwt provides a Nix flake with a binary package, overlay, and home-manager module.

## Quick Start

Add cwt as a flake input:

```nix
{
  inputs.cwt.url = "github:0dragosh/cwt";
}
```

### home-manager (recommended)

Import the module and enable `programs.cwt`:

```nix
{ inputs, ... }:
{
  imports = [ inputs.cwt.homeManagerModules.default ];

  programs.cwt = {
    enable = true;
    package = inputs.cwt.packages.${system}.default;
  };
}
```

### Standalone (without home-manager)

Add the package directly to your environment:

```nix
environment.systemPackages = [ inputs.cwt.packages.${system}.default ];
```

Or use the overlay:

```nix
nixpkgs.overlays = [ inputs.cwt.overlays.default ];
# then reference pkgs.cwt anywhere
```

## Configuration

The `settings` attribute is an attrset that maps directly to cwt's TOML config format. When non-empty, it is rendered to `~/.config/cwt/config.toml` via the Nix store.

### Example: basic setup

```nix
programs.cwt = {
  enable = true;
  settings = {
    worktree = {
      max_ephemeral = 20;
      auto_name = true;
    };
    session = {
      auto_launch = true;
    };
    ui = {
      theme = "default";
      show_diff_stat = true;
    };
  };
};
```

### Example: with setup script and handoff config

```nix
programs.cwt = {
  enable = true;
  settings = {
    worktree = {
      dir = ".claude/worktrees";
      max_ephemeral = 10;
    };
    setup = {
      script = "scripts/wt-setup.sh";
      timeout_secs = 60;
    };
    handoff = {
      method = "patch";
      warn_gitignore = true;
    };
  };
};
```

### Example: permission levels

```nix
programs.cwt = {
  enable = true;
  settings.session = {
    default_permission = "elevated";
    permissions.elevated.settings_override.sandbox = {
      enabled = true;
      autoAllowBashIfSandboxed = true;
      allowUnsandboxedCommands = false;
    };
    permissions.elevated_unsandboxed.extra_args = [
      "--dangerously-skip-permissions"
    ];
  };
};
```

The three permission levels are:

| Level | Key | Behavior |
|-------|-----|----------|
| Normal | `N` | Plain `claude` (default) |
| Elevated | `E` | Injects sandbox settings into `.claude/settings.local.json` |
| Elevated Unsandboxed | `U!` | Appends `--dangerously-skip-permissions` |

Press `m` in the TUI to cycle between levels at runtime. Note that `M` (save as default) is disabled when the config is Nix-managed since the file is a read-only symlink into the Nix store.

## How the Config File Works

When `settings` is non-empty, the module generates a TOML file in the Nix store and symlinks it to `~/.config/cwt/config.toml` via `xdg.configFile`. This means:

- The file is **read-only** — it is a symlink into the Nix store.
- Changes require updating your Nix configuration and running `home-manager switch`.
- Per-project config in `.cwt/config.toml` (inside a repo) still works and takes precedence over the global config, following cwt's normal project > global > default fallback chain.

If `settings` is left empty (the default), no config file is written, and cwt falls back to its built-in defaults.

## Development Shell

The flake also provides a dev shell for contributing to cwt:

```sh
nix develop github:0dragosh/cwt
```

This gives you the Rust toolchain, cargo-watch, cargo-edit, and runtime dependencies (git, tmux).
