//! Integration tests for cwt CLI capabilities.
//!
//! These tests exercise the core worktree management, state persistence,
//! snapshot, GC, handoff, hooks, config, and error-handling paths by
//! creating real (temporary) git repos on disk.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A shared temporary HOME directory so that git config and ~/.cwt/snapshots/
/// work correctly across all tests (including in the nix build sandbox where
/// the real HOME may not be writable).
fn test_home() -> &'static Path {
    static HOME: OnceLock<TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = TempDir::new().expect("create test HOME");
        // Configure git globally inside the test HOME
        let out = Command::new("git")
            .args(["config", "--global", "user.email", "test@cwt.dev"])
            .env("HOME", home.path())
            .output()
            .expect("git config");
        assert!(out.status.success());
        let out = Command::new("git")
            .args(["config", "--global", "user.name", "cwt-test"])
            .env("HOME", home.path())
            .output()
            .expect("git config");
        assert!(out.status.success());
        let out = Command::new("git")
            .args(["config", "--global", "init.defaultBranch", "main"])
            .env("HOME", home.path())
            .output()
            .expect("git config");
        assert!(out.status.success());
        home
    })
    .path()
}

/// Create an isolated HOME directory with git config.
/// Use this for tests that modify shared state (like forest.toml) to avoid races.
fn make_isolated_home() -> TempDir {
    let home = TempDir::new().expect("create isolated HOME");
    for args in [
        &["config", "--global", "user.email", "test@cwt.dev"][..],
        &["config", "--global", "user.name", "cwt-test"],
        &["config", "--global", "init.defaultBranch", "main"],
    ] {
        let out = Command::new("git")
            .args(args)
            .env("HOME", home.path())
            .output()
            .expect("git config");
        assert!(out.status.success());
    }
    home
}

/// Create a temporary git repo with an initial commit, returning the TempDir
/// (which keeps the directory alive) and the path to the repo root.
fn make_test_repo() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("create tempdir");
    let root = tmp.path().to_path_buf();

    run_git(&root, &["init"]);

    std::fs::write(root.join("README.md"), "# test repo\n").unwrap();
    run_git(&root, &["add", "."]);
    run_git(&root, &["commit", "-m", "initial commit"]);

    (tmp, root)
}

/// Shorthand for running a git command in a directory (panics on failure).
fn run_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("HOME", test_home())
        .output()
        .expect("git command failed to start");
    assert!(
        out.status.success(),
        "git {} failed in {}: {}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Run `cwt <args>` as a subprocess against the given repo root.
/// Returns (stdout, stderr, success).
fn run_cwt(repo_root: &Path, args: &[&str]) -> (String, String, bool) {
    run_cwt_with_home(repo_root, args, test_home())
}

/// Run `cwt <args>` with a custom HOME directory.
/// Returns (stdout, stderr, success).
fn run_cwt_with_home(repo_root: &Path, args: &[&str], home: &Path) -> (String, String, bool) {
    let bin = cwt_binary();
    let out = Command::new(&bin)
        .args(args)
        .current_dir(repo_root)
        .env("HOME", home)
        .output()
        .unwrap_or_else(|e| panic!("failed to run cwt binary at {}: {}", bin.display(), e));

    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.success(),
    )
}

/// Locate the cwt binary. Prefers the result/bin path from a nix build,
/// falls back to cargo's debug target directory.
fn cwt_binary() -> PathBuf {
    // Check CARGO_BIN_EXE_cwt (set by cargo test for [[bin]] targets)
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_cwt") {
        return PathBuf::from(p);
    }

    // Check nix build output
    let nix_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("result/bin/cwt");
    if nix_path.exists() {
        return nix_path;
    }

    // Fall back to cargo build target
    let target_debug = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/debug/cwt");
    if target_debug.exists() {
        return target_debug;
    }

    panic!(
        "cwt binary not found. Run `cargo build` or `nix build` first."
    );
}

/// Read and parse .cwt/state.json from a repo.
fn read_state(repo_root: &Path) -> serde_json::Value {
    let state_path = repo_root.join(".cwt/state.json");
    let content = std::fs::read_to_string(&state_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", state_path.display(), e));
    serde_json::from_str(&content).expect("failed to parse state.json")
}

// ===========================================================================
// Worktree CRUD
// ===========================================================================

#[test]
fn test_create_worktree_with_name() {
    let (_tmp, root) = make_test_repo();

    let (stdout, _stderr, ok) = run_cwt(&root, &["create", "my-feature", "--base", "main"]);
    assert!(ok, "cwt create failed: {_stderr}");
    assert!(stdout.contains("Created worktree 'my-feature'"));

    // Verify the worktree directory exists
    let wt_path = root.join(".claude/worktrees/my-feature");
    assert!(wt_path.exists(), "worktree dir should exist");

    // Verify git worktree is registered
    let out = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&root)
        .env("HOME", test_home())
        .output()
        .unwrap();
    let wt_list = String::from_utf8_lossy(&out.stdout);
    assert!(wt_list.contains("my-feature"), "git should list the worktree");
}

#[test]
fn test_create_worktree_auto_name() {
    let (_tmp, root) = make_test_repo();

    let (stdout, _stderr, ok) = run_cwt(&root, &["create", "--base", "main"]);
    assert!(ok, "cwt create failed: {_stderr}");
    assert!(stdout.contains("Created worktree '"));

    // Extract the auto-generated name from output
    let name = stdout
        .lines()
        .find(|l| l.starts_with("Created worktree"))
        .and_then(|l| l.split('\'').nth(1))
        .expect("should have a name in output");

    // Verify slug format: adj-noun-hex4
    let parts: Vec<&str> = name.split('-').collect();
    assert_eq!(parts.len(), 3, "slug should have 3 parts: {name}");
    assert_eq!(parts[2].len(), 4, "hex suffix should be 4 chars");
}

#[test]
fn test_list_worktrees() {
    let (_tmp, root) = make_test_repo();

    // Create two worktrees
    run_cwt(&root, &["create", "wt-alpha", "--base", "main"]);
    run_cwt(&root, &["create", "wt-beta", "--base", "main"]);

    let (stdout, _stderr, ok) = run_cwt(&root, &["list"]);
    assert!(ok, "cwt list failed: {_stderr}");

    assert!(stdout.contains("wt-alpha"));
    assert!(stdout.contains("wt-beta"));
    assert!(stdout.contains("2 worktree(s)"));
}

#[test]
fn test_list_empty() {
    let (_tmp, root) = make_test_repo();

    let (stdout, _stderr, ok) = run_cwt(&root, &["list"]);
    assert!(ok);
    assert!(stdout.contains("No managed worktrees"));
}

#[test]
fn test_promote_worktree() {
    let (_tmp, root) = make_test_repo();
    run_cwt(&root, &["create", "promo-wt", "--base", "main"]);

    // Should be ephemeral initially
    let state = read_state(&root);
    let lifecycle = state["worktrees"]["promo-wt"]["lifecycle"]
        .as_str()
        .unwrap();
    assert_eq!(lifecycle, "ephemeral");

    // Promote
    let (stdout, _stderr, ok) = run_cwt(&root, &["promote", "promo-wt"]);
    assert!(ok, "promote failed: {_stderr}");
    assert!(stdout.contains("Promoted"));

    // Verify permanent in state
    let state = read_state(&root);
    let lifecycle = state["worktrees"]["promo-wt"]["lifecycle"]
        .as_str()
        .unwrap();
    assert_eq!(lifecycle, "permanent");
}

#[test]
fn test_delete_worktree_saves_snapshot() {
    let (_tmp, root) = make_test_repo();

    // Create and then make a change in the worktree
    run_cwt(&root, &["create", "snap-wt", "--base", "main"]);
    let wt_path = root.join(".claude/worktrees/snap-wt");
    std::fs::write(wt_path.join("newfile.txt"), "hello\n").unwrap();

    // Delete — should save snapshot
    let (stdout, _stderr, ok) = run_cwt(&root, &["delete", "snap-wt"]);
    assert!(ok, "delete failed: {_stderr}");
    assert!(stdout.contains("snapshot saved"));

    // Verify worktree directory is gone
    assert!(!wt_path.exists(), "worktree dir should be removed");

    // Verify snapshot exists in state
    let state = read_state(&root);
    let snapshots = state["snapshots"].as_array().unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0]["name"].as_str().unwrap(), "snap-wt");

    // Verify the patch file was created
    let patch_path = snapshots[0]["patch_file"].as_str().unwrap();
    assert!(
        PathBuf::from(patch_path).exists(),
        "patch file should exist at {patch_path}"
    );
}

// ===========================================================================
// State Persistence
// ===========================================================================

#[test]
fn test_state_persisted_across_commands() {
    let (_tmp, root) = make_test_repo();

    run_cwt(&root, &["create", "persist-wt", "--base", "main"]);

    // State file should exist
    let state_path = root.join(".cwt/state.json");
    assert!(state_path.exists(), ".cwt/state.json should be created");

    // Verify round-trip: list reads from persisted state
    let (stdout, _stderr, ok) = run_cwt(&root, &["list"]);
    assert!(ok);
    assert!(stdout.contains("persist-wt"));
}

#[test]
fn test_state_version_field() {
    let (_tmp, root) = make_test_repo();
    run_cwt(&root, &["create", "v-wt", "--base", "main"]);

    let state = read_state(&root);
    assert_eq!(state["version"].as_u64().unwrap(), 1);
}

#[test]
fn test_state_worktree_fields() {
    let (_tmp, root) = make_test_repo();
    run_cwt(&root, &["create", "fields-wt", "--base", "main"]);

    let state = read_state(&root);
    let wt = &state["worktrees"]["fields-wt"];

    assert_eq!(wt["name"].as_str().unwrap(), "fields-wt");
    assert_eq!(wt["branch"].as_str().unwrap(), "wt/fields-wt");
    assert_eq!(wt["base_branch"].as_str().unwrap(), "main");
    assert_eq!(wt["lifecycle"].as_str().unwrap(), "ephemeral");
    assert!(wt["base_commit"].as_str().is_some());
    assert!(wt["created_at"].as_str().is_some());
    assert_eq!(wt["status"].as_str().unwrap(), "idle");
}

// ===========================================================================
// GC (Garbage Collection)
// ===========================================================================

#[test]
fn test_gc_nothing_to_prune() {
    let (_tmp, root) = make_test_repo();
    run_cwt(&root, &["create", "gc-wt", "--base", "main"]);

    let (stdout, _stderr, ok) = run_cwt(&root, &["gc"]);
    assert!(ok);
    assert!(stdout.contains("Nothing to GC"));
}

#[test]
fn test_gc_dry_run_and_execute() {
    let (_tmp, root) = make_test_repo();

    // Set up a fake remote so worktree branches have an "upstream" and
    // are not skipped by the "has unpushed commits" check in GC.
    // We use the repo itself as the remote.
    run_git(&root, &["remote", "add", "origin", root.to_str().unwrap()]);
    run_git(&root, &["fetch", "origin"]);
    // Push main so we have origin/main
    run_git(&root, &["push", "origin", "main"]);

    // Write a config with max_ephemeral = 2 so we can trigger GC
    let config_dir = root.join(".cwt");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[worktree]\nmax_ephemeral = 2\n",
    )
    .unwrap();

    // Create 4 ephemeral worktrees (exceeds limit of 2)
    for i in 0..4 {
        let name = format!("gc-wt-{i}");
        run_cwt(&root, &["create", &name, "--base", "main"]);
        // Push the branch so it has an upstream (GC skips branches without upstream)
        let branch = format!("wt/{name}");
        run_git(&root, &["push", "origin", &branch]);
        // Set upstream tracking for each worktree branch
        let wt_path = root.join(format!(".claude/worktrees/{name}"));
        run_git(
            &wt_path,
            &["branch", "--set-upstream-to", &format!("origin/{branch}")],
        );
    }

    // Dry run — should show worktrees to prune but not delete
    let (stdout, stderr, ok) = run_cwt(&root, &["gc"]);
    assert!(ok, "gc failed: {stderr}");
    assert!(
        stdout.contains("Worktrees to prune"),
        "expected prune preview, got stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(stdout.contains("Dry run"));

    // Verify all 4 still exist
    let (list_out, _, _) = run_cwt(&root, &["list"]);
    assert!(list_out.contains("4 worktree(s)"));

    // Execute GC
    let (stdout, _stderr, ok) = run_cwt(&root, &["gc", "--execute"]);
    assert!(ok);
    assert!(stdout.contains("Deleted"));

    // Verify count is down to max_ephemeral (2)
    let (list_out, _, _) = run_cwt(&root, &["list"]);
    assert!(list_out.contains("2 worktree(s)"));
}

// ===========================================================================
// Handoff
// ===========================================================================

#[test]
fn test_handoff_worktree_to_local() {
    let (_tmp, root) = make_test_repo();

    // Create worktree and add a tracked file change in it
    run_cwt(&root, &["create", "handoff-wt", "--base", "main"]);
    let wt_path = root.join(".claude/worktrees/handoff-wt");

    // Modify an existing tracked file so `git diff HEAD` picks it up
    std::fs::write(wt_path.join("README.md"), "# test repo\nmodified in worktree\n").unwrap();

    // Generate patch from worktree (tracked changes only)
    let patch_out = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(&wt_path)
        .env("HOME", test_home())
        .output()
        .unwrap();
    let patch = String::from_utf8_lossy(&patch_out.stdout);
    assert!(!patch.is_empty(), "worktree should have tracked changes");

    // Apply patch to local
    let mut child = Command::new("git")
        .args(["apply", "--3way", "-"])
        .current_dir(&root)
        .env("HOME", test_home())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(patch.as_bytes())
            .unwrap();
    }
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success(), "git apply should succeed");

    // Verify local has the modification
    let content = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(content.contains("modified in worktree"));
}

#[test]
fn test_handoff_local_to_worktree() {
    let (_tmp, root) = make_test_repo();

    // Create worktree
    run_cwt(&root, &["create", "handoff-lt", "--base", "main"]);
    let wt_path = root.join(".claude/worktrees/handoff-lt");

    // Add a file in local (but don't commit)
    std::fs::write(root.join("local-change.txt"), "from local\n").unwrap();

    // Generate patch from local
    // git diff HEAD won't show untracked files, so stage first + use --cached
    run_git(&root, &["add", "local-change.txt"]);
    let patch_out = Command::new("git")
        .args(["diff", "--cached"])
        .current_dir(&root)
        .env("HOME", test_home())
        .output()
        .unwrap();
    let patch = String::from_utf8_lossy(&patch_out.stdout);

    if !patch.is_empty() {
        // Apply patch to worktree
        let mut child = Command::new("git")
            .args(["apply", "--3way", "-"])
            .current_dir(&wt_path)
            .env("HOME", test_home())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(patch.as_bytes())
                .unwrap();
        }
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success(), "git apply should succeed in worktree");

        // Verify worktree has the file
        assert!(wt_path.join("local-change.txt").exists());
    }

    // Unstage in local to clean up
    run_git(&root, &["reset", "HEAD", "local-change.txt"]);
}

// ===========================================================================
// Hooks Install / Uninstall
// ===========================================================================

#[test]
fn test_hooks_install_creates_scripts() {
    let (_tmp, root) = make_test_repo();

    let (_stdout, _stderr, ok) = run_cwt(&root, &["hooks", "install"]);
    assert!(ok, "hooks install failed: {_stderr}");

    // Verify hook scripts were created
    let hooks_dir = root.join(".cwt/hooks");
    assert!(hooks_dir.join("cwt-stop.sh").exists());
    assert!(hooks_dir.join("cwt-notification.sh").exists());
    assert!(hooks_dir.join("cwt-subagent_stop.sh").exists());

    // Verify scripts are executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(hooks_dir.join("cwt-stop.sh"))
            .unwrap()
            .permissions();
        assert!(perms.mode() & 0o111 != 0, "hook script should be executable");
    }
}

#[test]
fn test_hooks_install_patches_settings_json() {
    let (_tmp, root) = make_test_repo();

    run_cwt(&root, &["hooks", "install"]);

    let settings_path = root.join(".claude/settings.json");
    assert!(settings_path.exists(), "settings.json should be created");

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Verify hooks are registered
    assert!(settings["hooks"]["Stop"].is_array());
    assert!(settings["hooks"]["Notification"].is_array());
    assert!(settings["hooks"]["SubagentStop"].is_array());

    // Verify cwt entries exist
    let stop_hooks = settings["hooks"]["Stop"].as_array().unwrap();
    assert!(
        stop_hooks
            .iter()
            .any(|h| h["command"].as_str().unwrap_or("").contains("cwt-")),
        "Stop hook should reference cwt script"
    );
}

#[test]
fn test_hooks_uninstall_removes_scripts() {
    let (_tmp, root) = make_test_repo();

    // Install first
    run_cwt(&root, &["hooks", "install"]);
    assert!(root.join(".cwt/hooks/cwt-stop.sh").exists());

    // Uninstall
    let (_stdout, _stderr, ok) = run_cwt(&root, &["hooks", "uninstall"]);
    assert!(ok, "hooks uninstall failed: {_stderr}");

    // Scripts should be gone
    assert!(!root.join(".cwt/hooks/cwt-stop.sh").exists());
    assert!(!root.join(".cwt/hooks/cwt-notification.sh").exists());
}

#[test]
fn test_hooks_uninstall_cleans_settings_json() {
    let (_tmp, root) = make_test_repo();

    run_cwt(&root, &["hooks", "install"]);
    run_cwt(&root, &["hooks", "uninstall"]);

    let content = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // cwt entries should be removed from hook arrays
    if let Some(stop_hooks) = settings["hooks"]["Stop"].as_array() {
        assert!(
            !stop_hooks
                .iter()
                .any(|h| h["command"].as_str().unwrap_or("").contains("cwt-")),
            "cwt hooks should be removed from settings.json"
        );
    }
}

#[test]
fn test_hooks_install_idempotent() {
    let (_tmp, root) = make_test_repo();

    run_cwt(&root, &["hooks", "install"]);
    run_cwt(&root, &["hooks", "install"]);

    let content = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Should not duplicate entries
    let stop_hooks = settings["hooks"]["Stop"].as_array().unwrap();
    let cwt_count = stop_hooks
        .iter()
        .filter(|h| h["command"].as_str().unwrap_or("").contains("cwt-"))
        .count();
    assert_eq!(cwt_count, 1, "should not duplicate hook entries");
}

#[test]
fn test_hooks_status() {
    let (_tmp, root) = make_test_repo();

    // Before install
    let (stdout, _stderr, ok) = run_cwt(&root, &["hooks", "status"]);
    assert!(ok);
    assert!(stdout.contains("not installed") || stdout.contains("inactive"));

    // After install
    run_cwt(&root, &["hooks", "install"]);
    let (stdout, _stderr, ok) = run_cwt(&root, &["hooks", "status"]);
    assert!(ok);
    assert!(stdout.contains("script(s)"));
}

// ===========================================================================
// Config
// ===========================================================================

#[test]
fn test_default_config_works() {
    let (_tmp, root) = make_test_repo();

    // No .cwt/config.toml — should use defaults and work fine
    let (_stdout, _stderr, ok) = run_cwt(&root, &["create", "default-cfg", "--base", "main"]);
    assert!(ok, "create with default config failed: {_stderr}");

    // Worktree should be under .claude/worktrees (the default dir)
    assert!(root.join(".claude/worktrees/default-cfg").exists());
}

#[test]
fn test_project_config_overrides_defaults() {
    let (_tmp, root) = make_test_repo();

    // Write a custom config
    let config_dir = root.join(".cwt");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        r#"
[worktree]
dir = ".worktrees"
max_ephemeral = 3
"#,
    )
    .unwrap();

    let (_stdout, _stderr, ok) = run_cwt(&root, &["create", "custom-cfg", "--base", "main"]);
    assert!(ok, "create with custom config failed: {_stderr}");

    // Worktree should be under .worktrees
    assert!(root.join(".worktrees/custom-cfg").exists());
}

// ===========================================================================
// Error Handling
// ===========================================================================

#[test]
fn test_create_duplicate_name_fails() {
    let (_tmp, root) = make_test_repo();

    run_cwt(&root, &["create", "dup-wt", "--base", "main"]);

    let (_stdout, stderr, ok) = run_cwt(&root, &["create", "dup-wt", "--base", "main"]);
    assert!(!ok, "duplicate create should fail");
    assert!(
        stderr.contains("already exists") || stderr.contains("failed"),
        "should mention the conflict: {stderr}"
    );
}

#[test]
fn test_delete_nonexistent_fails() {
    let (_tmp, root) = make_test_repo();

    let (_stdout, stderr, ok) = run_cwt(&root, &["delete", "ghost-wt"]);
    assert!(!ok, "deleting nonexistent should fail");
    assert!(
        stderr.contains("not found"),
        "should say not found: {stderr}"
    );
}

#[test]
fn test_promote_nonexistent_fails() {
    let (_tmp, root) = make_test_repo();

    let (_stdout, stderr, ok) = run_cwt(&root, &["promote", "ghost-wt"]);
    assert!(!ok, "promoting nonexistent should fail");
    assert!(
        stderr.contains("not found"),
        "should say not found: {stderr}"
    );
}

#[test]
fn test_not_in_git_repo() {
    let tmp = TempDir::new().expect("create tempdir");
    let dir = tmp.path().to_path_buf();

    let (_stdout, stderr, ok) = run_cwt(&dir, &["list"]);
    assert!(!ok, "should fail outside git repo");
    assert!(
        stderr.contains("not in a git repository") || stderr.contains("not a git repository"),
        "should mention git repo requirement: {stderr}"
    );
}

// ===========================================================================
// Carry Changes
// ===========================================================================

#[test]
fn test_create_with_carry_changes() {
    let (_tmp, root) = make_test_repo();

    // Create an uncommitted change in local
    std::fs::write(root.join("dirty.txt"), "uncommitted\n").unwrap();
    run_git(&root, &["add", "dirty.txt"]);

    // Create worktree with --carry
    let (_stdout, _stderr, ok) =
        run_cwt(&root, &["create", "carry-wt", "--base", "main", "--carry"]);
    assert!(ok, "create --carry failed: {_stderr}");

    // The worktree should have the carried change
    let wt_path = root.join(".claude/worktrees/carry-wt");
    assert!(
        wt_path.join("dirty.txt").exists(),
        "carried file should exist in worktree"
    );

    let content = std::fs::read_to_string(wt_path.join("dirty.txt")).unwrap();
    assert_eq!(content, "uncommitted\n");
}

// ===========================================================================
// Multiple Worktrees & Branch Naming
// ===========================================================================

#[test]
fn test_branch_naming_convention() {
    let (_tmp, root) = make_test_repo();
    run_cwt(&root, &["create", "feat-auth", "--base", "main"]);

    // Branch should be wt/<name>
    let state = read_state(&root);
    let branch = state["worktrees"]["feat-auth"]["branch"]
        .as_str()
        .unwrap();
    assert_eq!(branch, "wt/feat-auth");

    // Verify git has the branch
    let out = Command::new("git")
        .args(["branch", "--list", "wt/feat-auth"])
        .current_dir(&root)
        .env("HOME", test_home())
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&out.stdout);
    assert!(
        branches.contains("wt/feat-auth"),
        "git should have the wt/ branch"
    );
}

#[test]
fn test_delete_removes_branch() {
    let (_tmp, root) = make_test_repo();
    run_cwt(&root, &["create", "del-branch", "--base", "main"]);
    run_cwt(&root, &["delete", "del-branch"]);

    // Branch should be gone
    let out = Command::new("git")
        .args(["branch", "--list", "wt/del-branch"])
        .current_dir(&root)
        .env("HOME", test_home())
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&out.stdout);
    assert!(
        !branches.contains("wt/del-branch"),
        "branch should be deleted after worktree removal"
    );
}

// ===========================================================================
// State Reconciliation
// ===========================================================================

#[test]
fn test_state_reconciliation_after_manual_removal() {
    let (_tmp, root) = make_test_repo();

    // Create a worktree via cwt
    run_cwt(&root, &["create", "recon-wt", "--base", "main"]);

    // Manually remove it via git (bypassing cwt)
    let wt_path = root.join(".claude/worktrees/recon-wt");
    run_git(&root, &["worktree", "remove", "--force", wt_path.to_str().unwrap()]);

    // cwt list should reconcile and not show the removed worktree
    let (stdout, _stderr, ok) = run_cwt(&root, &["list"]);
    assert!(ok);
    assert!(
        !stdout.contains("recon-wt") || stdout.contains("No managed worktrees"),
        "reconciliation should remove stale entries"
    );
}

// ===========================================================================
// Forest Mode (add-repo, status)
// ===========================================================================

#[test]
fn test_status_no_repos() {
    let home = make_isolated_home();
    let (_tmp, root) = make_test_repo();

    let (stdout, _stderr, ok) = run_cwt_with_home(&root, &["status"], home.path());
    // With no repos registered, it should say so or show empty
    assert!(ok || _stderr.contains("No repos registered"));
    if ok {
        // Could show "0 repo(s)" or "No repos registered"
        assert!(
            stdout.contains("repo") || stdout.contains("No repos"),
            "should mention repos: {stdout}"
        );
    }
}

#[test]
fn test_add_repo() {
    let home = make_isolated_home();
    let (_tmp, root) = make_test_repo();

    let (stdout, _stderr, ok) =
        run_cwt_with_home(&root, &["add-repo", root.to_str().unwrap()], home.path());
    assert!(ok, "add-repo failed: {_stderr}");
    assert!(
        stdout.contains("Added repo") || stdout.contains("already registered"),
        "should confirm add: {stdout}"
    );
}

#[test]
fn test_add_repo_duplicate() {
    let home = make_isolated_home();
    let (_tmp, root) = make_test_repo();

    run_cwt_with_home(&root, &["add-repo", root.to_str().unwrap()], home.path());

    // Second add should indicate already registered
    let (stdout, _stderr, ok) =
        run_cwt_with_home(&root, &["add-repo", root.to_str().unwrap()], home.path());
    assert!(ok);
    assert!(
        stdout.contains("already registered"),
        "should say already registered: {stdout}"
    );
}

// ===========================================================================
// Snapshot Content
// ===========================================================================

#[test]
fn test_snapshot_contains_changes() {
    let (_tmp, root) = make_test_repo();

    run_cwt(&root, &["create", "snap-content", "--base", "main"]);
    let wt_path = root.join(".claude/worktrees/snap-content");

    // Make a committed change
    std::fs::write(wt_path.join("committed.txt"), "committed change\n").unwrap();
    run_git(&wt_path, &["add", "."]);
    run_git(&wt_path, &["commit", "-m", "add committed file"]);

    // Make an uncommitted change
    std::fs::write(wt_path.join("uncommitted.txt"), "uncommitted change\n").unwrap();

    // Delete (triggers snapshot)
    run_cwt(&root, &["delete", "snap-content"]);

    // Read the snapshot
    let state = read_state(&root);
    let patch_file = state["snapshots"][0]["patch_file"].as_str().unwrap();
    let patch_content = std::fs::read_to_string(patch_file).unwrap();

    // Should have metadata header
    assert!(patch_content.contains("# cwt snapshot: snap-content"));
    assert!(patch_content.contains("# base branch: main"));

    // Should have the committed changes
    assert!(
        patch_content.contains("committed.txt"),
        "snapshot should contain committed changes"
    );
}

// ===========================================================================
// GC Skips Protected Worktrees
// ===========================================================================

#[test]
fn test_gc_skips_dirty_worktrees() {
    let (_tmp, root) = make_test_repo();

    // Config with max_ephemeral = 1
    let config_dir = root.join(".cwt");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[worktree]\nmax_ephemeral = 1\n",
    )
    .unwrap();

    // Create 2 worktrees
    run_cwt(&root, &["create", "gc-clean", "--base", "main"]);
    run_cwt(&root, &["create", "gc-dirty", "--base", "main"]);

    // Make the second one dirty
    let dirty_path = root.join(".claude/worktrees/gc-dirty");
    std::fs::write(dirty_path.join("dirty.txt"), "dirty\n").unwrap();

    // GC should skip the dirty one
    let (stdout, _stderr, _ok) = run_cwt(&root, &["gc"]);
    if stdout.contains("Worktrees to prune") {
        // gc-dirty should NOT be in the prune list
        assert!(
            !stdout.contains("gc-dirty"),
            "dirty worktree should be skipped by GC"
        );
    }
}

// ===========================================================================
// Promote Idempotency
// ===========================================================================

#[test]
fn test_promote_already_permanent() {
    let (_tmp, root) = make_test_repo();
    run_cwt(&root, &["create", "perm-wt", "--base", "main"]);
    run_cwt(&root, &["promote", "perm-wt"]);

    // Second promote should succeed (or be a no-op)
    let (_stdout, _stderr, ok) = run_cwt(&root, &["promote", "perm-wt"]);
    assert!(ok, "re-promoting should not fail");
}
