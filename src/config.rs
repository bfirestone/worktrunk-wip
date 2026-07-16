//! Stage-mode resolution. Full worktrunk-config lookup lands in the
//! config task; this stub keeps the binary compiling with flag + default.

use crate::types::StageMode;
use worktrunk::git::Repository;

/// Resolve the effective stage mode: CLI flag → project `[wip] stage`
/// → user `[wip] stage` → `StageMode::All`.
pub fn resolve_stage_mode(flag: Option<StageMode>, _repo: &Repository) -> StageMode {
    flag.unwrap_or(StageMode::All)
}
