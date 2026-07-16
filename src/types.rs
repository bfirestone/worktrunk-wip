//! Shared contract types for wt-wip. Do NOT modify without updating the
//! approved plan (arc plan.08la3k).

use clap::ValueEnum;

/// What to stage before the wip checkpoint commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum StageMode {
    /// Stage everything: untracked files + unstaged tracked changes (default)
    All,
    /// Stage tracked changes only (like `git add -u`)
    Tracked,
    /// Stage nothing; commit only what's already in the index
    None,
}

/// Result of `wt-wip push`, serialized for `--format json`.
#[derive(serde::Serialize)]
pub struct PushResult {
    pub branch: String,
    pub remote: String,
    /// `Some(sha)` if a wip commit was created this run; `None` if nothing was staged.
    pub committed: Option<String>,
    pub outcome: PushOutcome,
    /// Commits pushed to the remote (0 when already up to date).
    pub commits_pushed: usize,
}

#[derive(serde::Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum PushOutcome {
    Pushed,
    UpToDate,
}

/// Result of `wt-wip pull`, serialized for `--format json`.
#[derive(serde::Serialize)]
pub struct PullResult {
    pub branch: String,
    pub remote: String,
    pub outcome: PullOutcome,
    /// Commits fast-forwarded into the local branch (0 when already up to date).
    pub commits_pulled: usize,
}

#[derive(serde::Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum PullOutcome {
    FastForwarded,
    UpToDate,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Contract assertions ---
    // These verify the design spec (arc plan.08la3k). Do NOT modify
    // without updating the approved plan.

    #[test]
    fn outcome_serialization_contract() {
        assert_eq!(serde_json::to_string(&PushOutcome::Pushed).unwrap(), "\"pushed\"");
        assert_eq!(serde_json::to_string(&PushOutcome::UpToDate).unwrap(), "\"up-to-date\"");
        assert_eq!(
            serde_json::to_string(&PullOutcome::FastForwarded).unwrap(),
            "\"fast-forwarded\""
        );
        assert_eq!(serde_json::to_string(&PullOutcome::UpToDate).unwrap(), "\"up-to-date\"");
    }
}
