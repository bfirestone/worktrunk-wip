//! Stage-mode resolution: CLI flag → project `[wip] stage` → user
//! `[wip] stage` → default. Generic TOML lookup of only the `[wip]`
//! table — never a typed deserialization of worktrunk's schema, so
//! upstream config evolution cannot break us. Missing or malformed
//! values degrade to the default.

use std::path::{Path, PathBuf};

use crate::types::StageMode;
use worktrunk::git::Repository;

/// Resolve the effective stage mode: CLI flag → project `[wip] stage`
/// → user `[wip] stage` → `StageMode::All`.
pub fn resolve_stage_mode(flag: Option<StageMode>, repo: &Repository) -> StageMode {
    if let Some(mode) = flag {
        return mode;
    }
    project_config_path(repo)
        .and_then(|p| stage_from_file(&p))
        .or_else(|| user_config_path().and_then(|p| stage_from_file(&p)))
        .unwrap_or(StageMode::All)
}

/// `<repo toplevel>/.config/wt.toml` — worktrunk's project config, resolved
/// via worktrunk's own lookup (handles bare repos and the
/// `WORKTRUNK_PROJECT_CONFIG_PATH` test-isolation override).
fn project_config_path(repo: &Repository) -> Option<PathBuf> {
    repo.project_config_path().ok().flatten()
}

/// Worktrunk's user config path, resolved via worktrunk's own discovery
/// order (`WORKTRUNK_CONFIG_PATH` → platform config dir).
fn user_config_path() -> Option<PathBuf> {
    worktrunk::config::config_path()
}

/// Generic lookup of `stage` in the `[wip]` table. Any failure → `None`.
fn stage_from_file(path: &Path) -> Option<StageMode> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: toml::Value = text.parse().ok()?;
    match value.get("wip")?.get("stage")?.as_str()? {
        "all" => Some(StageMode::All),
        "tracked" => Some(StageMode::Tracked),
        "none" => Some(StageMode::None),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::with_env_config;
    use std::fs;
    use std::process::Command;

    fn temp_repo() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let ok = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let repo = Repository::at(dir.path()).unwrap();
        (dir, repo)
    }

    #[test]
    fn flag_beats_everything() {
        let (dir, repo) = temp_repo();
        fs::create_dir_all(dir.path().join(".config")).unwrap();
        fs::write(
            dir.path().join(".config/wt.toml"),
            "[wip]\nstage = \"none\"\n",
        )
        .unwrap();
        assert_eq!(
            resolve_stage_mode(Some(StageMode::Tracked), &repo),
            StageMode::Tracked
        );
    }

    #[test]
    fn project_config_applies_without_flag() {
        let (dir, repo) = temp_repo();
        fs::create_dir_all(dir.path().join(".config")).unwrap();
        fs::write(
            dir.path().join(".config/wt.toml"),
            "[wip]\nstage = \"tracked\"\n",
        )
        .unwrap();
        assert_eq!(resolve_stage_mode(None, &repo), StageMode::Tracked);
    }

    #[test]
    fn missing_config_defaults_to_all() {
        let (dir, repo) = temp_repo();
        // Point the user-config layer at a path that doesn't exist so this
        // test isn't at the mercy of the real user's `~/.config/worktrunk`.
        let result = with_env_config(&dir.path().join("no-such-config.toml"), || {
            resolve_stage_mode(None, &repo)
        });
        assert_eq!(result, StageMode::All);
    }

    #[test]
    fn malformed_config_degrades_to_default() {
        let (dir, repo) = temp_repo();
        fs::create_dir_all(dir.path().join(".config")).unwrap();
        fs::write(
            dir.path().join(".config/wt.toml"),
            "[wip]\nstage = \"banana\"\n",
        )
        .unwrap();
        let result = with_env_config(&dir.path().join("no-such-config.toml"), || {
            resolve_stage_mode(None, &repo)
        });
        assert_eq!(result, StageMode::All);
    }

    #[test]
    fn user_config_applies_without_project_config() {
        let (_dir, repo) = temp_repo();
        let user_dir = tempfile::tempdir().unwrap();
        let user_config = user_dir.path().join("config.toml");
        fs::write(&user_config, "[wip]\nstage = \"tracked\"\n").unwrap();
        let result = with_env_config(&user_config, || resolve_stage_mode(None, &repo));
        assert_eq!(result, StageMode::Tracked);
    }

    #[test]
    fn stage_from_file_parses_all_modes() {
        let dir = tempfile::tempdir().unwrap();
        for (val, want) in [
            ("all", StageMode::All),
            ("tracked", StageMode::Tracked),
            ("none", StageMode::None),
        ] {
            let p = dir.path().join("c.toml");
            fs::write(&p, format!("[wip]\nstage = \"{val}\"\n")).unwrap();
            assert_eq!(stage_from_file(&p), Some(want));
        }
    }
}
