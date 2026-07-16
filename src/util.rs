//! Shared helpers used by both push and pull.

use worktrunk::git::Repository;

/// Build the auto-generated wip commit message:
/// `wip @ <hostname> — <RFC3339 timestamp>`.
///
/// Diagnostic only (squashed at `wt merge`); the hostname tag identifies
/// which machine made each checkpoint.
pub fn wip_message() -> String {
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    format!("wip @ {host} — {ts}")
}

/// Resolve the remote to sync `branch` against. Reuses the branch's
/// configured push remote (`branch.<name>.pushRemote` →
/// `remote.pushDefault` → `branch.<name>.remote`), falling back to
/// `origin`.
pub fn resolve_remote(repo: &Repository, branch: &str) -> anyhow::Result<String> {
    Ok(repo
        .branch(branch)
        .push_remote()
        .unwrap_or_else(|| "origin".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Contract assertions ---

    #[test]
    fn wip_message_contract() {
        let _: fn() -> String = wip_message;
        let msg = wip_message();
        assert!(msg.starts_with("wip @ "), "got: {msg}");
        assert!(msg.contains(" — "), "got: {msg}");
    }
}
