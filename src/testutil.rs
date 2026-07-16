//! Shared test harness for `src/*.rs` unit tests. Compiled only under
//! `#[cfg(test)]` (see the `mod testutil` decl in `main.rs`).
//!
//! Push, pull, and config tests all mutate process-global state — the
//! process cwd and the `WORKTRUNK_CONFIG_PATH` env var — and `cargo test`
//! runs them concurrently by default. A lock private to one module can't
//! prevent another module's tests from racing it, so every test that
//! touches either piece of global state must share this one lock.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

/// Serializes every test (across push/pull/config) that touches the
/// process cwd or `WORKTRUNK_CONFIG_PATH`.
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git").args(args).current_dir(dir).status().unwrap();
    assert!(st.success(), "git {args:?} failed in {dir:?}");
}

pub(crate) fn configure(clone: &Path) {
    git(clone, &["config", "user.name", "test"]);
    git(clone, &["config", "user.email", "test@example.com"]);
    git(clone, &["config", "commit.gpgsign", "false"]);
}

/// Run `f` with the process cwd set to `dir` and `WORKTRUNK_CONFIG_PATH`
/// pointed at a nonexistent path (isolating any in-process config lookup
/// from the developer's real user config), holding `TEST_LOCK` for the
/// duration. Restores both before releasing the lock.
pub(crate) fn in_dir<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = TEST_LOCK.lock().unwrap();
    let prev_dir = std::env::current_dir().unwrap();
    let prev_config = std::env::var_os("WORKTRUNK_CONFIG_PATH");
    std::env::set_current_dir(dir).unwrap();
    unsafe { std::env::set_var("WORKTRUNK_CONFIG_PATH", dir.join("no-such-config.toml")) };
    let out = f();
    std::env::set_current_dir(prev_dir).unwrap();
    restore_env_config(prev_config);
    out
}

/// Run `f` with `WORKTRUNK_CONFIG_PATH` set to `path`, holding `TEST_LOCK`
/// for the duration (serializes against the cwd/env mutations `in_dir`
/// performs for push/pull tests too). Restores the previous value (or
/// absence) before releasing.
pub(crate) fn with_env_config<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = TEST_LOCK.lock().unwrap();
    let prev_config = std::env::var_os("WORKTRUNK_CONFIG_PATH");
    unsafe { std::env::set_var("WORKTRUNK_CONFIG_PATH", path) };
    let out = f();
    restore_env_config(prev_config);
    out
}

fn restore_env_config(prev: Option<OsString>) {
    match prev {
        Some(v) => unsafe { std::env::set_var("WORKTRUNK_CONFIG_PATH", v) },
        None => unsafe { std::env::remove_var("WORKTRUNK_CONFIG_PATH") },
    }
}
