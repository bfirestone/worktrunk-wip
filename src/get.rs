//! `wt-wip get` — materialize a branch's WIP into a worktree, then pull.
//!
//! The mirror of the "sit down at machine B" moment: a branch you pushed WIP
//! to from another machine may not exist as a worktree here yet. `get`
//! provisions the worktrunk worktree for it and fast-forwards it to the
//! remote in one step.
//!
//! Unlike push/pull — which run standalone in any git checkout — `get` is
//! deliberately worktrunk-native: it shells out to `wt switch` so the
//! worktree lands where worktrunk's own placement, config, and hooks put it.
//! It composes over the safe primitives and adds no history-touching git of
//! its own: provisioning is `wt switch` (no branch rewrite), and the sync is
//! the same fast-forward-only [`pull_in`] the `pull` verb uses.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context};
use color_print::cformat;
use worktrunk::git::Repository;
use worktrunk::styling::{eprintln, info_message, progress_message, success_message};

use crate::pull::pull_in;
use crate::types::{PullOutcome, PullResult};
use crate::util::resolve_remote;

/// Result of `wt-wip get`, serialized for `--format json`.
///
/// Kept local to this module for the prototype rather than added to the
/// plan-locked `types.rs` contract. If `get` graduates, promote this
/// alongside `PushResult`/`PullResult` (and add it to the JSON-contract
/// test) as a deliberate plan update.
#[derive(serde::Serialize)]
pub struct GetResult {
    pub branch: String,
    pub remote: String,
    /// Absolute path of the worktree `get` landed on.
    pub worktree_path: String,
    /// `true` if `get` provisioned the worktree this run; `false` if it
    /// already existed.
    pub created: bool,
    /// Outcome of the fast-forward step inside the worktree.
    pub pull_outcome: PullOutcome,
    /// Commits fast-forwarded into the worktree (0 when already up to date).
    pub commits_pulled: usize,
}

pub fn get(branch: &str, create: bool) -> anyhow::Result<GetResult> {
    let repo = Repository::current()?;

    // 1. Find the worktree for `branch`, or provision one the worktrunk way.
    //    Idempotent: re-running `get` on a branch that already has a worktree
    //    just brings it up to date.
    let (worktree_path, created) = match repo.worktree_for_branch(branch)? {
        Some(path) => {
            eprintln!(
                "{}",
                info_message(cformat!("Worktree for <bold>{branch}</> already exists"))
            );
            (path, false)
        }
        None => (provision_worktree(branch, create)?, true),
    };

    // 2. Fast-forward inside that worktree with the safe pull primitive.
    //    `Repository::at` roots git commands in the worktree dir, so this
    //    never touches the caller's cwd.
    //
    //    Only pull when the branch has an upstream: a branch tracking a remote
    //    (the "receive WIP" case) has one, but a freshly created `--create`
    //    branch does not — and `pull_in`'s `git fetch <remote> <branch>` would
    //    fail on the missing remote ref. No upstream means nothing to
    //    fast-forward from, so report a clean no-op instead.
    let worktree_repo = Repository::at(&worktree_path)?;
    let current = worktree_repo.require_current_branch("wip get")?;
    let pulled = if worktree_repo.branch(&current).upstream()?.is_some() {
        pull_in(&worktree_repo)?
    } else {
        eprintln!(
            "{}",
            info_message(cformat!(
                "<bold>{current}</> has no upstream yet; nothing to fast-forward"
            ))
        );
        PullResult {
            remote: resolve_remote(&worktree_repo, &current)?,
            branch: current,
            outcome: PullOutcome::UpToDate,
            commits_pulled: 0,
        }
    };

    // 3. Point the user at the worktree — a subprocess can't cd their shell,
    //    so the path is the actionable output.
    eprintln!(
        "{}",
        success_message(cformat!("Ready at <bold>{}</>", worktree_path.display()))
    );

    Ok(GetResult {
        branch: pulled.branch,
        remote: pulled.remote,
        worktree_path: worktree_path.display().to_string(),
        created,
        pull_outcome: pulled.outcome,
        commits_pulled: pulled.commits_pulled,
    })
}

/// Provision a worktrunk worktree for `branch` by delegating to `wt switch`.
///
/// We shell out to the `wt` binary rather than `git worktree add` on purpose:
/// worktrunk decides where the worktree lives and runs its own hooks/config,
/// and reproducing that here would just be a worse copy. That makes `get`
/// (unlike push/pull) require `wt` on PATH — an acceptable trade for a verb
/// whose whole job is worktrunk-native provisioning.
///
/// We always *track first*: plain `wt switch <branch>` builds a worktree that
/// tracks the branch when it already exists locally or on the remote — the
/// normal "receive WIP pushed from another machine" case.
///
/// `--create` is opt-in and only takes effect as a *fallback* when tracking
/// fails (the branch exists nowhere). It is deliberately never passed to a
/// branch that already exists on the remote: worktrunk's `switch --create`
/// would warn and build a divergent new-branch-from-base instead of tracking,
/// which is exactly the WIP-loss `get` exists to prevent. So the only path to
/// `switch --create` is "plain switch failed *and* the caller asked for it."
fn provision_worktree(branch: &str, create: bool) -> anyhow::Result<PathBuf> {
    eprintln!(
        "{}",
        progress_message(cformat!(
            "Provisioning worktree for <bold>{branch}</> via wt switch..."
        ))
    );

    // Attempt 1: track an existing (local or remote) branch.
    if wt_switch(&["switch", branch])? {
        return locate_worktree(branch);
    }

    // Attempt 2: only if the caller opted in, create the branch fresh. Reached
    // only when tracking failed, so this can't shadow a remote branch.
    if create {
        eprintln!(
            "{}",
            info_message(cformat!(
                "<bold>{branch}</> not found to track; creating it (--create)"
            ))
        );
        if wt_switch(&["switch", "--create", branch])? {
            return locate_worktree(branch);
        }
        bail!("`wt switch --create {branch}` failed; cannot provision a worktree for `get`");
    }

    bail!(
        "Branch `{branch}` was not found locally or on the remote. If it is new, \
         re-run with `--create` to start it here."
    )
}

/// Run one `wt switch` variant, inheriting stderr so worktrunk's own progress
/// and error output reaches the user. Returns whether it succeeded; only the
/// spawn failure (e.g. `wt` missing from PATH) is a hard error.
fn wt_switch(args: &[&str]) -> anyhow::Result<bool> {
    Ok(Command::new("wt")
        .args(args)
        .status()
        .context("Failed to run `wt switch` — is worktrunk (`wt`) installed and on PATH?")?
        .success())
}

/// Re-discover the freshly provisioned worktree's path. A fresh `Repository`
/// is required because `list_worktrees` is cached per handle, so the
/// pre-provision handle can't see the worktree we just made.
fn locate_worktree(branch: &str) -> anyhow::Result<PathBuf> {
    let repo = Repository::current()?;
    repo.worktree_for_branch(branch)?.with_context(|| {
        format!("`wt switch {branch}` reported success but no worktree for `{branch}` was found")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{configure, git, in_dir};
    use std::fs;
    use std::path::Path;

    /// bare remote + clone `a` (on `main`) with a linked worktree already
    /// checked out on `feat`, both branches pushed. Returns the clone root
    /// and the `feat` worktree path.
    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let a = dir.path().join("a");
        git(dir.path(), &["init", "--bare", remote.to_str().unwrap()]);
        git(
            dir.path(),
            &["clone", remote.to_str().unwrap(), a.to_str().unwrap()],
        );
        configure(&a);
        git(&a, &["checkout", "-b", "main"]);
        fs::write(a.join("README.md"), "seed\n").unwrap();
        git(&a, &["add", "-A"]);
        git(&a, &["commit", "-m", "seed"]);
        git(&a, &["push", "-u", "origin", "main"]);
        // A `feat` branch with its own linked worktree, pushed to the remote.
        git(&a, &["branch", "feat"]);
        git(&a, &["push", "-u", "origin", "feat"]);
        let feat_wt = dir.path().join("a-feat");
        git(&a, &["worktree", "add", feat_wt.to_str().unwrap(), "feat"]);
        configure(&feat_wt);
        (dir, a, feat_wt)
    }

    /// Advance `feat` on the remote from an unrelated clone.
    fn advance_remote_feat(root: &Path) {
        let other = root.join("other");
        git(
            root,
            &[
                "clone",
                root.join("remote.git").to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        configure(&other);
        git(&other, &["checkout", "feat"]);
        fs::write(other.join("f.txt"), "x\n").unwrap();
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-m", "advance feat"]);
        git(&other, &["push", "origin", "feat"]);
    }

    #[test]
    fn get_reuses_existing_worktree_and_fast_forwards() {
        let (dir, a, feat_wt) = setup();
        advance_remote_feat(dir.path());
        // Runs from the `main` worktree; finds the existing `feat` worktree
        // (no `wt` needed) and fast-forwards it.
        let result = in_dir(&a, || get("feat", false)).unwrap();
        assert!(!result.created, "worktree already existed");
        assert_eq!(result.pull_outcome, PullOutcome::FastForwarded);
        assert_eq!(result.commits_pulled, 1);
        assert!(
            feat_wt.join("f.txt").exists(),
            "the worktree on disk was fast-forwarded"
        );
    }

    #[test]
    fn get_on_up_to_date_worktree_is_a_no_op() {
        let (_dir, a, _feat_wt) = setup();
        let result = in_dir(&a, || get("feat", false)).unwrap();
        assert!(!result.created);
        assert_eq!(result.pull_outcome, PullOutcome::UpToDate);
        assert_eq!(result.commits_pulled, 0);
    }
}
