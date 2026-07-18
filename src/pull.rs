//! `wt-wip pull` — fetch and fast-forward the current branch.
//! Fast-forward only: refuses divergence rather than rewriting anything.

use anyhow::Context;
use color_print::cformat;
use worktrunk::git::Repository;
use worktrunk::styling::{eprintln, info_message, progress_message, success_message};

use crate::types::{PullOutcome, PullResult};
use crate::util::resolve_remote;

pub fn pull() -> anyhow::Result<PullResult> {
    pull_in(&Repository::current()?)
}

/// Fetch and fast-forward `repo`'s current branch. Split out from [`pull`]
/// so `wt-wip get` can run the exact same safe primitive inside a freshly
/// provisioned worktree — any [`Repository`], not just the process cwd.
pub fn pull_in(repo: &Repository) -> anyhow::Result<PullResult> {
    let branch = repo.require_current_branch("wip pull")?;
    let remote = resolve_remote(repo, &branch)?;

    // 1. Fetch the branch from the remote.
    eprintln!(
        "{}",
        progress_message(cformat!(
            "Fetching <bold>{branch}</> from <bold>{remote}</>..."
        ))
    );
    repo.run_command(&["fetch", &remote, "--end-of-options", &branch])
        .with_context(|| format!("Failed to fetch {branch} from {remote}"))?;

    let remote_ref = format!("{remote}/{branch}");

    // 2. How many commits is the remote ahead? 0 → already up to date.
    let commits_pulled: usize = repo
        .run_command(&[
            "rev-list",
            "--count",
            "--end-of-options",
            &format!("HEAD..{remote_ref}"),
        ])
        .with_context(|| format!("Failed to count commits behind {remote_ref}"))?
        .trim()
        .parse()
        .context("Unexpected non-numeric rev-list output")?;

    if commits_pulled == 0 {
        eprintln!(
            "{}",
            info_message(cformat!("Already up to date with <bold>{remote_ref}</>"))
        );
        return Ok(PullResult {
            branch,
            remote,
            outcome: PullOutcome::UpToDate,
            commits_pulled: 0,
        });
    }

    // 3. Advance with the safe primitive: merge with ff-only. It refuses
    //    (and changes nothing) when the local branch has diverged or when
    //    uncommitted changes would be overwritten — surface git's message
    //    rather than clobbering anything.
    repo.run_command(&["merge", "--ff-only", "--end-of-options", &remote_ref])
        .with_context(|| {
            format!(
                "Cannot fast-forward {branch} to {remote_ref} — the local branch has diverged (you may have unpushed commits) or has conflicting uncommitted changes. Run `wt wip push` or reconcile manually."
            )
        })?;

    eprintln!(
        "{}",
        success_message(cformat!(
            "Fast-forwarded <bold>{branch}</> to <bold>{remote_ref}</> ({commits_pulled} commits)"
        ))
    );

    Ok(PullResult {
        branch,
        remote,
        outcome: PullOutcome::FastForwarded,
        commits_pulled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{configure, git, in_dir};
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    /// bare remote + two configured clones, seed commit pushed to main
    fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
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
        git(
            dir.path(),
            &["clone", remote.to_str().unwrap(), b.to_str().unwrap()],
        );
        configure(&b);
        (dir, a, b)
    }

    fn commit_and_push(clone: &Path, file: &str) {
        fs::write(clone.join(file), "content\n").unwrap();
        git(clone, &["add", "-A"]);
        git(clone, &["commit", "-m", "advance"]);
        git(clone, &["push", "origin", "main"]);
    }

    #[test]
    fn pull_fast_forwards_and_counts() {
        let (_dir, a, b) = setup();
        commit_and_push(&a, "one.txt");
        let result = in_dir(&b, pull).unwrap();
        assert_eq!(result.outcome, PullOutcome::FastForwarded);
        assert_eq!(result.commits_pulled, 1);
        assert!(b.join("one.txt").exists());
    }

    #[test]
    fn pull_up_to_date_is_a_no_op() {
        let (_dir, _a, b) = setup();
        let result = in_dir(&b, pull).unwrap();
        assert_eq!(result.outcome, PullOutcome::UpToDate);
        assert_eq!(result.commits_pulled, 0);
    }

    #[test]
    fn pull_preserves_uncommitted_non_conflicting_changes() {
        let (_dir, a, b) = setup();
        commit_and_push(&a, "one.txt");
        fs::write(b.join("local-scratch.txt"), "precious\n").unwrap();
        let result = in_dir(&b, pull).unwrap();
        assert_eq!(result.outcome, PullOutcome::FastForwarded);
        assert_eq!(
            fs::read_to_string(b.join("local-scratch.txt")).unwrap(),
            "precious\n"
        );
    }

    #[test]
    fn diverged_pull_fails_cleanly_naming_wip_push() {
        let (_dir, a, b) = setup();
        commit_and_push(&a, "one.txt");
        // b diverges with its own local commit.
        fs::write(b.join("mine.txt"), "local\n").unwrap();
        git(&b, &["add", "-A"]);
        git(&b, &["commit", "-m", "local divergence"]);
        let head_before = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&b)
            .output()
            .unwrap();
        // `PullResult` (foundation type in `src/types.rs`) derives only
        // `Serialize`, not `Debug`, so `Result::unwrap_err` (which requires
        // `T: Debug`) can't be used here; extract the error manually instead.
        let err = match in_dir(&b, pull) {
            Err(e) => e,
            Ok(_) => panic!("expected pull() to fail on divergence"),
        };
        assert!(format!("{err:#}").contains("wt wip push"), "got: {err:#}");
        let head_after = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&b)
            .output()
            .unwrap();
        assert_eq!(
            head_before.stdout, head_after.stdout,
            "pull must not move HEAD on failure"
        );
    }
}
