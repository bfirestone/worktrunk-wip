//! `wt-wip push` — stage, wip-commit, and push. Append-only: never
//! rewrites history, never overwrites remote commits.

use anyhow::Context;
use color_print::cformat;
use worktrunk::git::Repository;
use worktrunk::styling::{eprintln, info_message, progress_message, success_message};

use crate::config::resolve_stage_mode;
use crate::types::{PushOutcome, PushResult, StageMode};
use crate::util::{resolve_remote, wip_message};

pub fn push(stage: Option<StageMode>, message: Option<String>) -> anyhow::Result<PushResult> {
    let repo = Repository::current()?;
    let branch = repo.require_current_branch("wip push")?;
    let remote = resolve_remote(&repo, &branch)?;
    let stage_mode = resolve_stage_mode(stage, &repo);

    // 1. Stage changes according to the resolved stage mode.
    match stage_mode {
        StageMode::All => {
            repo.run_command(&["add", "-A"])
                .context("Failed to stage changes")?;
        }
        StageMode::Tracked => {
            repo.run_command(&["add", "-u"])
                .context("Failed to stage tracked changes")?;
        }
        StageMode::None => {
            // Stage nothing; commit only what's already in the index.
        }
    }

    // 2. Commit only when something is actually staged.
    //    `diff --cached --quiet` exits 0 when the index is clean, non-zero
    //    when there are staged changes — so run_command_check returns false
    //    ("there ARE staged changes") when the index is dirty.
    let has_staged = !repo.run_command_check(&["diff", "--cached", "--quiet"])?;
    let committed = if has_staged {
        let msg = message.unwrap_or_else(wip_message);
        eprintln!("{}", progress_message("Committing wip checkpoint..."));
        repo.run_command(&["commit", "-m", &msg])
            .context("Failed to create wip commit")?;
        let sha = repo.run_command(&["rev-parse", "HEAD"])?.trim().to_string();
        Some(sha)
    } else {
        None
    };

    // 3. Count commits ahead of upstream for reporting. This is advisory
    //    only (it just picks the Pushed vs UpToDate message below) and runs
    //    before the push, so a count hiccup must never abort a viable push —
    //    keep these fallbacks soft.
    let has_upstream = repo.branch(&branch).upstream()?.is_some();
    let commits_pushed = if has_upstream {
        repo.run_command(&["rev-list", "--count", "@{upstream}..HEAD"])
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0)
    } else {
        // First push: count only what the remote is missing. `--not
        // --remotes=<remote>` subtracts everything already reachable from
        // the remote's tracking refs, so shared mainline history isn't
        // counted as "sent" (plain `--count HEAD` would report the branch's
        // entire ancestry). If the count fails, fall back to whether this
        // run made a checkpoint rather than a made-up constant.
        let exclude = format!("--remotes={remote}");
        repo.run_command(&["rev-list", "--count", "HEAD", "--not", &exclude])
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(usize::from(committed.is_some()))
    };

    // 4. Push (append-only). Set upstream on the first push with -u.
    //    `--end-of-options` fences the ref/branch-derived positional args
    //    per worktrunk's own run_command convention.
    eprintln!(
        "{}",
        progress_message(cformat!(
            "Pushing <bold>{branch}</> to <bold>{remote}</>..."
        ))
    );
    let push_args: Vec<&str> = if has_upstream {
        vec!["push", "--end-of-options", &remote, &branch]
    } else {
        vec!["push", "-u", "--end-of-options", &remote, &branch]
    };
    // A non-fast-forward rejection means the remote has commits we don't
    // have. `.with_context` preserves the underlying error so git's stderr
    // renders below the message.
    repo.run_command(&push_args).with_context(|| {
        format!(
            "Push to {remote}/{branch} was rejected — the remote has commits you don't have. Run `wt wip pull` first."
        )
    })?;

    let outcome = if commits_pushed > 0 {
        eprintln!(
            "{}",
            success_message(cformat!("Pushed <bold>{branch}</> to <bold>{remote}</>"))
        );
        PushOutcome::Pushed
    } else {
        eprintln!(
            "{}",
            info_message(cformat!(
                "Already up to date with <bold>{remote}/{branch}</>"
            ))
        );
        PushOutcome::UpToDate
    };

    Ok(PushResult {
        branch,
        remote,
        committed,
        outcome,
        commits_pushed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{configure, git, in_dir};
    use std::fs;
    use std::process::Command;

    /// bare remote + configured clone with one pushed commit on main
    fn setup() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let clone = dir.path().join("clone");
        git(dir.path(), &["init", "--bare", remote.to_str().unwrap()]);
        git(
            dir.path(),
            &["clone", remote.to_str().unwrap(), clone.to_str().unwrap()],
        );
        configure(&clone);
        git(&clone, &["checkout", "-b", "main"]);
        fs::write(clone.join("README.md"), "seed\n").unwrap();
        git(&clone, &["add", "-A"]);
        git(&clone, &["commit", "-m", "seed"]);
        git(&clone, &["push", "-u", "origin", "main"]);
        (dir, clone)
    }

    #[test]
    fn push_commits_and_pushes_changes() {
        let (_dir, clone) = setup();
        fs::write(clone.join("wip.txt"), "wip\n").unwrap();
        let result = in_dir(&clone, || push(None, None)).unwrap();
        assert!(result.committed.is_some());
        assert_eq!(result.outcome, PushOutcome::Pushed);
        assert!(result.commits_pushed >= 1);
    }

    #[test]
    fn clean_tree_up_to_date_makes_no_commit() {
        let (_dir, clone) = setup();
        let result = in_dir(&clone, || push(None, None)).unwrap();
        assert!(result.committed.is_none());
        assert_eq!(result.outcome, PushOutcome::UpToDate);
        assert_eq!(result.commits_pushed, 0);
    }

    #[test]
    fn stage_tracked_excludes_untracked_files() {
        let (_dir, clone) = setup();
        fs::write(clone.join("untracked.txt"), "new\n").unwrap();
        let result = in_dir(&clone, || push(Some(StageMode::Tracked), None)).unwrap();
        // Nothing tracked changed → no commit; untracked file survives.
        assert!(result.committed.is_none());
        assert!(clone.join("untracked.txt").exists());
    }

    #[test]
    fn custom_message_overrides_generated_one() {
        let (_dir, clone) = setup();
        fs::write(clone.join("wip.txt"), "wip\n").unwrap();
        in_dir(&clone, || push(None, Some("checkpoint: custom".into()))).unwrap();
        let out = Command::new("git")
            .args(["log", "-1", "--format=%s"])
            .current_dir(&clone)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "checkpoint: custom"
        );
    }

    #[test]
    fn rejected_push_names_wip_pull() {
        let (dir, clone) = setup();
        // Second clone advances the remote so the first clone's push is rejected.
        let other = dir.path().join("other");
        git(
            dir.path(),
            &[
                "clone",
                dir.path().join("remote.git").to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        configure(&other);
        fs::write(other.join("other.txt"), "x\n").unwrap();
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-m", "other machine"]);
        git(&other, &["push", "origin", "main"]);

        fs::write(clone.join("wip.txt"), "wip\n").unwrap();
        // `PushResult` (foundation type in `src/types.rs`) derives only
        // `Serialize`, not `Debug`, so `Result::unwrap_err` (which requires
        // `T: Debug`) can't be used here; extract the error manually instead.
        let err = match in_dir(&clone, || push(None, None)) {
            Err(e) => e,
            Ok(_) => panic!("expected push() to fail on rejected push"),
        };
        assert!(format!("{err:#}").contains("wt wip pull"), "got: {err:#}");
    }

    #[test]
    fn first_push_sets_upstream() {
        let (_dir, clone) = setup();
        // A brand-new local branch has no upstream yet — exercises the
        // `-u` first-push path.
        git(&clone, &["checkout", "-b", "feature"]);
        fs::write(clone.join("wip.txt"), "wip\n").unwrap();
        let result = in_dir(&clone, || push(None, None)).unwrap();
        assert_eq!(result.outcome, PushOutcome::Pushed);
        // Exactly 1: only the wip checkpoint is new to the remote. The seed
        // commit shared with main must not be counted (regression: the old
        // `rev-list --count HEAD` reported the branch's entire ancestry).
        assert_eq!(result.commits_pushed, 1);
        let out = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "feature@{upstream}"])
            .current_dir(&clone)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "origin/feature"
        );
    }
}
