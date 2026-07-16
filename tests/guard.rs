//! Data-safety invariant: wt-wip must never force-push or rewind history.
//! Enforced by scanning the crate source for forbidden git substrings.

use std::fs;
use std::path::Path;

#[test]
fn src_has_no_destructive_git_commands() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = String::new();
    for entry in fs::read_dir(&dir).expect("src dir exists") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") {
            sources.push_str(&fs::read_to_string(&path).unwrap());
        }
    }
    assert!(
        !sources.contains("--force"),
        "wt-wip must never force-push (append-only invariant)"
    );
    let rewind = ["re", "set"].concat();
    assert!(
        !sources.contains(&rewind),
        "wt-wip must never rewind history (use merge --ff-only instead)"
    );
}
