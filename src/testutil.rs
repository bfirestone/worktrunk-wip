//! Shared test harness for `src/*.rs` unit tests. Compiled only under
//! `#[cfg(test)]` (see the `mod testutil` decl in `main.rs`).
//!
//! Push, pull, and config tests all mutate process-global state — the
//! process cwd and the `WORKTRUNK_CONFIG_PATH` env var — and `cargo test`
//! runs them concurrently by default. A lock private to one module can't
//! prevent another module's tests from racing it, so every test that
//! touches either piece of global state must share this one lock.
//!
//! Restoration is RAII-based (an `EnvGuard` whose `Drop` restores cwd and
//! `WORKTRUNK_CONFIG_PATH`) and lock acquisition tolerates poison, so a
//! panicking test still restores global state and releases the lock
//! cleanly instead of cascading into unrelated failures elsewhere.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Serializes every test (across push/pull/config) that touches the
/// process cwd or `WORKTRUNK_CONFIG_PATH`.
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?} failed in {dir:?}");
}

pub(crate) fn configure(clone: &Path) {
    git(clone, &["config", "user.name", "test"]);
    git(clone, &["config", "user.email", "test@example.com"]);
    git(clone, &["config", "commit.gpgsign", "false"]);
}

/// Acquire `TEST_LOCK`, tolerating poison left by an earlier test's panic
/// — a prior failure must not cascade into `PoisonError` failures in
/// unrelated tests.
fn lock() -> MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Restores the previous cwd and `WORKTRUNK_CONFIG_PATH` on drop — runs
/// even if the closure passed to `in_dir`/`with_env_config` panics, since
/// this guard is dropped during unwind. Holds `TEST_LOCK` (via `_lock`)
/// until restoration is complete, releasing it only once state is back to
/// how the test found it.
struct EnvGuard {
    prev_dir: Option<PathBuf>,
    prev_config: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(dir) = self.prev_dir.take() {
            // Non-panicking: a panic here during unwind would abort the
            // process and mask the original test failure. With the prior
            // cwd now captured under the lock (see `in_dir`/`with_env_config`
            // below), `dir` is always the stable repo dir the test started
            // in, so this is belt-and-suspenders.
            if let Err(e) = std::env::set_current_dir(&dir) {
                eprintln!("testutil: failed to restore cwd to {dir:?}: {e}");
            }
        }
        match self.prev_config.take() {
            Some(v) => unsafe { std::env::set_var("WORKTRUNK_CONFIG_PATH", v) },
            None => unsafe { std::env::remove_var("WORKTRUNK_CONFIG_PATH") },
        }
    }
}

/// Run `f` with the process cwd set to `dir` and `WORKTRUNK_CONFIG_PATH`
/// pointed at a nonexistent path (isolating any in-process config lookup
/// from the developer's real user config), holding `TEST_LOCK` for the
/// duration. Both are restored by `EnvGuard::drop`, so restoration still
/// happens if `f` panics.
///
/// The lock is acquired *before* the previous cwd/env are captured — a
/// struct literal's field initializers run in textual order, so capturing
/// state first (as an inline `EnvGuard { prev_dir: ..., _lock: lock() }`)
/// would read another thread's in-flight cwd mutation instead of the
/// stable value this thread is about to restore to.
pub(crate) fn in_dir<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
    let lock = lock();
    let _guard = EnvGuard {
        prev_dir: Some(std::env::current_dir().expect("cwd exists at capture")),
        prev_config: std::env::var_os("WORKTRUNK_CONFIG_PATH"),
        _lock: lock,
    };
    std::env::set_current_dir(dir).unwrap();
    unsafe { std::env::set_var("WORKTRUNK_CONFIG_PATH", dir.join("no-such-config.toml")) };
    f()
}

/// Run `f` with `WORKTRUNK_CONFIG_PATH` set to `path`, holding `TEST_LOCK`
/// for the duration (serializes against the cwd/env mutations `in_dir`
/// performs for push/pull tests too). Restored by `EnvGuard::drop`, so
/// restoration still happens if `f` panics.
///
/// As in `in_dir`, the lock is acquired before the previous env value is
/// captured, so the capture can't observe another thread's in-flight
/// mutation.
pub(crate) fn with_env_config<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    let lock = lock();
    let _guard = EnvGuard {
        prev_dir: None,
        prev_config: std::env::var_os("WORKTRUNK_CONFIG_PATH"),
        _lock: lock,
    };
    unsafe { std::env::set_var("WORKTRUNK_CONFIG_PATH", path) };
    f()
}
