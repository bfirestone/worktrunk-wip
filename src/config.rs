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

/// `<repo toplevel>/.config/wt.toml` — worktrunk's project config.
fn project_config_path(repo: &Repository) -> Option<PathBuf> {
    let root = repo.run_command(&["rev-parse", "--show-toplevel"]).ok()?;
    Some(PathBuf::from(root.trim()).join(".config").join("wt.toml"))
}

/// Worktrunk's user config discovery order:
/// `$WORKTRUNK_CONFIG_PATH` → `$XDG_CONFIG_HOME/worktrunk/config.toml`
/// → `$HOME/.config/worktrunk/config.toml`.
fn user_config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WORKTRUNK_CONFIG_PATH") {
        return Some(PathBuf::from(p));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("worktrunk").join("config.toml"));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("worktrunk").join("config.toml"))
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
        fs::write(dir.path().join(".config/wt.toml"), "[wip]\nstage = \"none\"\n").unwrap();
        assert_eq!(resolve_stage_mode(Some(StageMode::Tracked), &repo), StageMode::Tracked);
    }

    #[test]
    fn project_config_applies_without_flag() {
        let (dir, repo) = temp_repo();
        fs::create_dir_all(dir.path().join(".config")).unwrap();
        fs::write(dir.path().join(".config/wt.toml"), "[wip]\nstage = \"tracked\"\n").unwrap();
        assert_eq!(resolve_stage_mode(None, &repo), StageMode::Tracked);
    }

    #[test]
    fn missing_config_defaults_to_all() {
        let (_dir, repo) = temp_repo();
        assert_eq!(resolve_stage_mode(None, &repo), StageMode::All);
    }

    #[test]
    fn malformed_config_degrades_to_default() {
        let (dir, repo) = temp_repo();
        fs::create_dir_all(dir.path().join(".config")).unwrap();
        fs::write(dir.path().join(".config/wt.toml"), "[wip]\nstage = \"banana\"\n").unwrap();
        assert_eq!(resolve_stage_mode(None, &repo), StageMode::All);
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
