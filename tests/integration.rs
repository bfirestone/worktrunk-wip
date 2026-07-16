//! End-to-end tests: drive the compiled wt-wip binary against local
//! bare remotes. No network, no developer-config leakage.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git").args(args).current_dir(dir).status().unwrap();
    assert!(st.success(), "git {args:?} failed in {dir:?}");
}

fn configure(clone: &Path) {
    git(clone, &["config", "user.name", "test"]);
    git(clone, &["config", "user.email", "test@example.com"]);
    git(clone, &["config", "commit.gpgsign", "false"]);
}

/// bare remote + two clones (a: seeded + upstream set; b: fresh clone of the seed)
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote.git");
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    git(dir.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(dir.path(), &["clone", remote.to_str().unwrap(), a.to_str().unwrap()]);
    configure(&a);
    git(&a, &["checkout", "-b", "main"]);
    fs::write(a.join("README.md"), "seed\n").unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-m", "seed"]);
    git(&a, &["push", "-u", "origin", "main"]);
    git(dir.path(), &["clone", remote.to_str().unwrap(), b.to_str().unwrap()]);
    configure(&b);
    (dir, a, b)
}

fn wip(dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wt-wip"));
    cmd.args(args)
        .current_dir(dir)
        .env("WORKTRUNK_CONFIG_PATH", "/nonexistent-wt-wip-test");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "expected success.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn head_subject(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn bare_invocation_pushes_and_pull_round_trips() {
    let (_dir, a, b) = setup();
    fs::write(a.join("wip.txt"), "from machine a\n").unwrap();
    assert_ok(&wip(&a, &[], &[]));                    // bare = push shorthand
    assert!(head_subject(&a).starts_with("wip @ "));
    assert_ok(&wip(&b, &["pull"], &[]));
    assert_eq!(fs::read_to_string(b.join("wip.txt")).unwrap(), "from machine a\n");
}

#[test]
fn explicit_push_subcommand_works() {
    let (_dir, a, _b) = setup();
    fs::write(a.join("wip.txt"), "x\n").unwrap();
    assert_ok(&wip(&a, &["push"], &[]));
    assert!(head_subject(&a).starts_with("wip @ "));
}

#[test]
fn message_override_and_stage_tracked() {
    let (_dir, a, _b) = setup();
    // untracked file + tracked change; tracked mode must only take the latter
    fs::write(a.join("untracked.txt"), "new\n").unwrap();
    fs::write(a.join("README.md"), "seed changed\n").unwrap();
    assert_ok(&wip(&a, &["push", "--stage", "tracked", "-m", "checkpoint: custom"], &[]));
    assert_eq!(head_subject(&a), "checkpoint: custom");
    // untracked.txt was not committed: still untracked in status
    let st = Command::new("git")
        .args(["status", "--porcelain", "--", "untracked.txt"])
        .current_dir(&a)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&st.stdout).starts_with("??"));
}

#[test]
fn project_config_sets_stage_and_flag_beats_it() {
    let (_dir, a, _b) = setup();
    fs::create_dir_all(a.join(".config")).unwrap();
    fs::write(a.join(".config/wt.toml"), "[wip]\nstage = \"none\"\n").unwrap();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "-m", "add project config"]);
    // stage=none from config: an unstaged change must NOT be committed
    fs::write(a.join("README.md"), "unstaged edit\n").unwrap();
    assert_ok(&wip(&a, &[], &[]));
    assert_eq!(head_subject(&a), "add project config");
    // --stage all beats the config: now it IS committed
    assert_ok(&wip(&a, &["push", "--stage", "all"], &[]));
    assert!(head_subject(&a).starts_with("wip @ "));
}

#[test]
fn user_config_via_worktrunk_config_path_env() {
    let (dir, a, _b) = setup();
    let user_cfg = dir.path().join("user-config.toml");
    fs::write(&user_cfg, "[wip]\nstage = \"none\"\n").unwrap();
    fs::write(a.join("README.md"), "unstaged edit\n").unwrap();
    let out = wip(&a, &[], &[("WORKTRUNK_CONFIG_PATH", user_cfg.to_str().unwrap())]);
    assert_ok(&out);
    // stage=none → no wip commit was created for the unstaged edit
    assert_eq!(head_subject(&a), "seed");
}

#[test]
fn json_output_schema_push_and_pull() {
    let (_dir, a, b) = setup();
    fs::write(a.join("wip.txt"), "x\n").unwrap();
    let out = wip(&a, &["push", "--format", "json"], &[]);
    assert_ok(&out);
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("push stdout is one JSON object");
    assert_eq!(v["branch"], "main");
    assert_eq!(v["outcome"], "pushed");
    assert!(v["committed"].is_string());
    assert!(v["commits_pushed"].as_u64().unwrap() >= 1);

    let out = wip(&b, &["pull", "--format", "json"], &[]);
    assert_ok(&out);
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("pull stdout is one JSON object");
    assert_eq!(v["outcome"], "fast-forwarded");
    assert!(v["commits_pulled"].as_u64().unwrap() >= 1);
}

#[test]
fn up_to_date_both_ways_exit_zero() {
    let (_dir, a, _b) = setup();
    let out = wip(&a, &["push", "--format", "json"], &[]);
    assert_ok(&out);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["outcome"], "up-to-date");
    assert_eq!(v["committed"], serde_json::Value::Null);
    let out = wip(&a, &["pull", "--format", "json"], &[]);
    assert_ok(&out);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["outcome"], "up-to-date");
}

#[test]
fn divergence_fails_safely_in_both_directions() {
    let (_dir, a, b) = setup();
    // a advances the remote
    fs::write(a.join("a.txt"), "a\n").unwrap();
    assert_ok(&wip(&a, &[], &[]));
    // b diverges locally
    fs::write(b.join("b.txt"), "b\n").unwrap();
    git(&b, &["add", "-A"]);
    git(&b, &["commit", "-m", "local divergence"]);

    // push from b is rejected, names wt wip pull, exits non-zero
    let out = wip(&b, &["push"], &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("wt wip pull"));

    // pull into b refuses, names wt wip push, HEAD unmoved
    let before = head_subject(&b);
    let out = wip(&b, &["pull"], &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("wt wip push"));
    assert_eq!(head_subject(&b), before);
}

#[test]
fn pull_preserves_dirty_non_conflicting_tree() {
    let (_dir, a, b) = setup();
    fs::write(a.join("new.txt"), "x\n").unwrap();
    assert_ok(&wip(&a, &[], &[]));
    fs::write(b.join("scratch.txt"), "precious\n").unwrap();
    assert_ok(&wip(&b, &["pull"], &[]));
    assert_eq!(fs::read_to_string(b.join("scratch.txt")).unwrap(), "precious\n");
}
