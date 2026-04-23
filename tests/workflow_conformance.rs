//! Conformance checks for the sidecar workflow model.
//!
//! These tests exercise real temporary git repositories through the cwt binary
//! and compare the observable state against the same invariants the Verus
//! sidecar models abstractly.

use proptest::prelude::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct TestRepo {
    _repo_dir: TempDir,
    home_dir: TempDir,
    root: PathBuf,
}

impl TestRepo {
    fn new() -> Self {
        let repo_dir = TempDir::new().expect("create repo dir");
        let home_dir = TempDir::new().expect("create home dir");
        configure_git(home_dir.path());

        let root = repo_dir.path().to_path_buf();
        run_git(home_dir.path(), &root, &["init"]);
        std::fs::write(root.join("README.md"), "# conformance repo\n").expect("write README");
        run_git(home_dir.path(), &root, &["add", "."]);
        run_git(home_dir.path(), &root, &["commit", "-m", "initial commit"]);

        Self {
            _repo_dir: repo_dir,
            home_dir,
            root,
        }
    }

    fn run_cwt(&self, args: &[&str]) -> (String, String, bool) {
        let out = Command::new(cwt_binary())
            .args(args)
            .current_dir(&self.root)
            .env("HOME", self.home_dir.path())
            .env("XDG_CONFIG_HOME", self.home_dir.path().join(".config"))
            .output()
            .unwrap_or_else(|e| panic!("failed to run cwt {}: {e}", args.join(" ")));

        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
            out.status.success(),
        )
    }

    fn run_cwt_ok(&self, args: &[&str]) -> String {
        let (stdout, stderr, ok) = self.run_cwt(args);
        assert!(
            ok,
            "cwt {} failed in {}:\n{}",
            args.join(" "),
            self.root.display(),
            stderr
        );
        stdout
    }

    fn git(&self, dir: &Path, args: &[&str]) {
        run_git(self.home_dir.path(), dir, args);
    }

    fn worktree_path(&self, name: &str) -> PathBuf {
        self.root.join(".claude/worktrees").join(name)
    }
}

#[derive(Clone, Debug)]
enum WorkflowOp {
    Create(usize),
    Promote(usize),
    Delete(usize),
}

fn workflow_op() -> impl Strategy<Value = WorkflowOp> {
    prop_oneof![
        (0usize..3).prop_map(WorkflowOp::Create),
        (0usize..3).prop_map(WorkflowOp::Promote),
        (0usize..3).prop_map(WorkflowOp::Delete),
    ]
}

fn workflow_name(index: usize) -> &'static str {
    ["alpha", "beta", "gamma"][index]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4,
        max_shrink_iters: 0,
        .. ProptestConfig::default()
    })]

    #[test]
    fn create_delete_promote_round_trips_follow_model(
        ops in proptest::collection::vec(workflow_op(), 1..8)
    ) {
        let repo = TestRepo::new();
        let mut model = BTreeMap::<String, String>::new();

        for op in ops {
            match op {
                WorkflowOp::Create(index) => {
                    let name = workflow_name(index);
                    let (_stdout, stderr, ok) = repo.run_cwt(&["create", name, "--base", "main"]);
                    if model.contains_key(name) {
                        prop_assert!(!ok, "duplicate create for {name} should fail");
                        prop_assert!(
                            stderr.contains("already exists") || stderr.contains("failed"),
                            "duplicate create should explain the conflict, got: {stderr}"
                        );
                    } else {
                        prop_assert!(ok, "create {name} should succeed: {stderr}");
                        model.insert(name.to_string(), "ephemeral".to_string());
                    }
                }
                WorkflowOp::Promote(index) => {
                    let name = workflow_name(index);
                    let (_stdout, _stderr, ok) = repo.run_cwt(&["promote", name]);
                    if let Some(lifecycle) = model.get_mut(name) {
                        prop_assert!(ok, "promote {name} should succeed");
                        *lifecycle = "permanent".to_string();
                    } else {
                        prop_assert!(!ok, "promote of absent {name} should fail");
                    }
                }
                WorkflowOp::Delete(index) => {
                    let name = workflow_name(index);
                    let (_stdout, _stderr, ok) = repo.run_cwt(&["delete", name]);
                    if model.remove(name).is_some() {
                        prop_assert!(ok, "delete {name} should succeed");
                        prop_assert!(
                            !repo.worktree_path(name).exists(),
                            "delete should remove worktree directory for {name}"
                        );
                        prop_assert!(
                            snapshot_names(&repo.root).iter().any(|snapshot| snapshot == name),
                            "delete should retain snapshot metadata for {name}"
                        );
                    } else {
                        prop_assert!(!ok, "delete of absent {name} should fail");
                    }
                }
            }

            prop_assert_eq!(worktree_lifecycles(&repo.root), model.clone());
        }
    }
}

#[test]
fn gc_preview_skips_permanent_running_dirty_and_unpushed_worktrees() {
    let repo = TestRepo::new();
    std::fs::create_dir_all(repo.root.join(".cwt")).expect("create .cwt");
    std::fs::write(
        repo.root.join(".cwt/config.toml"),
        "[worktree]\nmax_ephemeral = 1\n",
    )
    .expect("write config");

    repo.git(
        &repo.root,
        &["remote", "add", "origin", repo.root.to_str().unwrap()],
    );
    repo.git(&repo.root, &["fetch", "origin"]);
    repo.git(&repo.root, &["push", "origin", "main"]);

    for name in [
        "gc-clean-old",
        "gc-permanent",
        "gc-running",
        "gc-dirty",
        "gc-unpushed",
        "gc-clean-new",
    ] {
        repo.run_cwt_ok(&["create", name, "--base", "main"]);
    }

    for name in [
        "gc-clean-old",
        "gc-permanent",
        "gc-running",
        "gc-dirty",
        "gc-clean-new",
    ] {
        let branch = format!("wt/{name}");
        repo.git(&repo.root, &["push", "origin", &branch]);
        repo.git(
            &repo.worktree_path(name),
            &["branch", "--set-upstream-to", &format!("origin/{branch}")],
        );
    }

    repo.run_cwt_ok(&["promote", "gc-permanent"]);
    set_worktree_status(&repo.root, "gc-running", "running");
    std::fs::write(repo.worktree_path("gc-dirty").join("dirty.txt"), "dirty\n")
        .expect("dirty worktree");

    let stdout = repo.run_cwt_ok(&["gc"]);

    assert!(stdout.contains("gc-clean-old"));
    assert!(stdout.contains("gc-clean-new"));
    for protected in ["gc-permanent", "gc-running", "gc-dirty", "gc-unpushed"] {
        assert!(
            !stdout.contains(protected),
            "GC preview must not select protected worktree {protected}; stdout was:\n{stdout}"
        );
    }
}

#[test]
fn verification_sidecar_files_are_present_and_document_trusted_boundaries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let assumptions = std::fs::read_to_string(root.join("verification/ASSUMPTIONS.md"))
        .expect("verification assumptions doc should exist");
    for boundary in [
        "git command truth",
        "patch application semantics",
        "filesystem atomicity",
        "tmux/session facts",
        "wall-clock timestamps",
        "Verus/Rust compiler trust",
    ] {
        assert!(
            assumptions.contains(boundary),
            "ASSUMPTIONS.md should document trusted boundary: {boundary}"
        );
    }

    let workflow = std::fs::read_to_string(root.join("verification/workflow.rs"))
        .expect("Verus workflow model should exist");
    for proof_name in [
        "lemma_create_adds_fresh_idle_ephemeral",
        "lemma_delete_removes_exactly_one_after_snapshot",
        "lemma_promote_is_idempotent",
        "lemma_gc_preview_selects_only_safe_excess",
        "lemma_restore_preserves_snapshot_metadata",
        "lemma_handoff_success_only_mutates_target",
    ] {
        assert!(
            workflow.contains(proof_name),
            "Verus workflow model should contain proof {proof_name}"
        );
    }

    let script_path = root.join("scripts/verify-verus.sh");
    let script = std::fs::read_to_string(&script_path).expect("Verus runner should exist");
    assert!(
        script.contains("verification"),
        "runner should scan verification/"
    );
    assert!(script.contains("verus"), "runner should invoke verus");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(script_path)
            .expect("stat Verus runner")
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0, "Verus runner should be executable");
    }
}

fn configure_git(home: &Path) {
    for args in [
        &["config", "--global", "user.email", "test@cwt.dev"][..],
        &["config", "--global", "user.name", "cwt-test"],
        &["config", "--global", "init.defaultBranch", "main"],
    ] {
        let out = Command::new("git")
            .args(args)
            .env("HOME", home)
            .output()
            .expect("git config should start");
        assert!(
            out.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn run_git(home: &Path, dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("HOME", home)
        .output()
        .unwrap_or_else(|e| panic!("git {} failed to start: {e}", args.join(" ")));
    assert!(
        out.status.success(),
        "git {} failed in {}:\n{}",
        args.join(" "),
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn cwt_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_cwt") {
        return PathBuf::from(path);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_debug = manifest_dir.join("target/debug/cwt");
    if target_debug.exists() {
        return target_debug;
    }

    panic!("cwt binary not found. Run `cargo build` first.");
}

fn read_state(root: &Path) -> serde_json::Value {
    let path = root.join(".cwt/state.json");
    if !path.exists() {
        return serde_json::json!({
            "worktrees": {},
            "snapshots": [],
        });
    }

    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&content).expect("state should be valid JSON")
}

fn worktree_lifecycles(root: &Path) -> BTreeMap<String, String> {
    let state = read_state(root);
    state["worktrees"]
        .as_object()
        .unwrap_or_else(|| panic!("state worktrees should be an object"))
        .iter()
        .map(|(name, wt)| {
            (
                name.clone(),
                wt["lifecycle"]
                    .as_str()
                    .unwrap_or_else(|| panic!("worktree {name} should have lifecycle"))
                    .to_string(),
            )
        })
        .collect()
}

fn snapshot_names(root: &Path) -> Vec<String> {
    let state = read_state(root);
    state["snapshots"]
        .as_array()
        .unwrap_or_else(|| panic!("state snapshots should be an array"))
        .iter()
        .map(|snapshot| {
            snapshot["name"]
                .as_str()
                .expect("snapshot should have name")
                .to_string()
        })
        .collect()
}

fn set_worktree_status(root: &Path, name: &str, status: &str) {
    let state_path = root.join(".cwt/state.json");
    let content = std::fs::read_to_string(&state_path).expect("read state");
    let mut state: serde_json::Value = serde_json::from_str(&content).expect("parse state");
    state["worktrees"][name]["status"] = serde_json::Value::String(status.to_string());
    std::fs::write(
        &state_path,
        serde_json::to_string_pretty(&state).expect("serialize state"),
    )
    .expect("write state");
}
