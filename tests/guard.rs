//! Data-safety invariant: wt-wip must never force-push or rewind history.
//! Enforced by scanning the crate's *runtime* source for forbidden git
//! tokens. Test modules are excluded — fixtures legitimately use verbs
//! (`checkout -b`, `worktree remove`) the shipped binary must never run —
//! by taking everything before a file's first `#[cfg(test)]` marker, which
//! in this crate's layout is always the trailing test module.
//!
//! This is a tripwire, not a proof: it catches a forbidden verb typed as a
//! contiguous token, not one assembled at runtime. The behavioral tests
//! (divergence refusal, dirty-tree survival) are the actual proof of the
//! safety guarantees; this guard makes the careless case a build failure.

use std::fs;
use std::path::Path;

/// Forbidden substrings and why each is destructive. Quoted entries (e.g.
/// `"\"clean\""`) match the git verb as a string literal argument, so prose
/// in comments ("a clean no-op") doesn't trip them; bare entries match
/// anywhere, comments and help text included.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "--force",
        "force-push of any flavor breaks the append-only invariant",
    ),
    ("reset", "rewinds history (pull is merge --ff-only instead)"),
    ("\"checkout\"", "checkout can discard working-tree changes"),
    ("\"clean\"", "clean deletes untracked files"),
    ("\"stash\"", "stash drop/clear can lose WIP"),
    ("update-ref", "raw ref surgery can rewind branches"),
    ("--delete", "deleting remote refs loses pushed WIP"),
    ("--hard", "hard modes discard work"),
];

#[test]
fn runtime_src_has_no_destructive_git_commands() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in fs::read_dir(&dir).expect("src dir exists") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let runtime = source
            .split("#[cfg(test)]")
            .next()
            .expect("split always yields at least one piece");
        for (token, why) in FORBIDDEN {
            assert!(
                !runtime.contains(token),
                "{} contains forbidden `{token}`: {why}",
                path.display()
            );
        }
    }
}
