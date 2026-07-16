<!-- arc-review: id=plan.08la3k -->
---
title: wt-wip — external worktrunk subcommand for append-only cross-machine WIP sync
date: "2026-07-15"
project: wt-wip
status: in_review
tags:
    - arc
    - design-spec
arc_review:
    kind: legacy
    id: plan.08la3k
---
# wt-wip — external worktrunk subcommand for append-only cross-machine WIP sync

**Date:** 2026-07-15
**Status:** Draft
**Origin:** Port of the `wt sync` feature from the worktrunk fork branch
`feat/sync-cross-machine` (`~/devspace/personal/github/worktrunk/.worktrees/feat-sync-cross-machine`)
into a standalone external subcommand, following the worktrunk extension model
(<https://worktrunk.dev/extending/#custom-subcommands>) and the
[`worktrunk-sync`](https://github.com/pablospe/worktrunk-sync) precedent.

## Problem

Moving in-progress work between machines today means hand-rolled
`git add -A && git commit -m wip && git push` on one side and a careful
fetch/fast-forward on the other — easy to get wrong (force-push, `reset --hard`,
clobbered uncommitted changes). The fork solved this in-tree as `wt sync`, but
in-tree means maintaining a fork. Worktrunk's git-style dispatch (any `wt-<name>`
executable on PATH becomes `wt <name>`) lets the same feature ship as an
independent crate with zero upstream buy-in.

## Decision summary

| Decision | Choice |
|---|---|
| Delivery | Standalone Rust crate `worktrunk-wip`, binary `wt-wip`, invoked as `wt wip` |
| Approach | Depend on the published `worktrunk` lib (0.68+) — near-verbatim port of the fork's `push.rs`/`pull.rs`; matches `worktrunk-sync` structure |
| CLI shape | `push`/`pull` subcommands **plus** bare `wt wip` as push shorthand (accepts push's flags) |
| Scope carried over | `--stage all\|tracked\|none`, `-m/--message`, `--format text\|json` |
| Scope dropped | `[branch]` positional arg — current branch only (removes non-current-branch pull plumbing) |
| Config | `[wip] stage` read from worktrunk's own config files (project `.config/wt.toml`, then user config), generic TOML table lookup — no typed schema coupling. `[wip]` only — no fallback to the fork's `[sync]` key |
| Safety model | Unchanged from fork: append-only; never `push --force*` / `reset --hard`; pull is `merge --ff-only`; enforced by a source-scan guard test |
| Distribution (v1) | Local install only: `cargo install --path .`. crates.io publishing is a later, separate decision |
| Remote | Public GitHub repo (`gh repo create --public`) at bootstrap — session workflow requires a push remote |

## Architecture

### Packaging & layout

Package `worktrunk-wip`, `[[bin]] wt-wip`, edition 2021, MIT license — parallel
to `worktrunk-sync`/`wt-sync`. Lives in `worktrunk-wip/` (this repo).
Install: `cargo install worktrunk-wip`.

```
worktrunk-wip/
├── Cargo.toml           # package worktrunk-wip, [[bin]] wt-wip
├── src/
│   ├── main.rs          # clap CLI + dispatch (bare → push)
│   ├── types.rs         # StageMode, PushResult/PullResult + outcome enums (T0 contracts)
│   ├── util.rs          # wip_message(), resolve_remote() (T0 contracts)
│   ├── config.rs        # [wip] stage resolution from worktrunk config files
│   ├── push.rs          # ported from fork commands/sync/push.rs
│   └── pull.rs          # ported from fork commands/sync/pull.rs
├── tests/
│   ├── integration.rs   # end-to-end push/pull against tempfile bare-remote + clones
│   └── guard.rs         # destructive-git-substring source scan (ported from fork)
├── .github/workflows/ci.yaml
├── README.md
├── .gitignore  .pre-commit-config.yaml  .typos.toml
└── Cargo.lock           # committed (binary crate)
```

**Dependencies:** `worktrunk` (pinned minor), `anyhow`, `clap` (derive),
`color-print`, `gethostname`, `chrono`, `serde` + `serde_json`, `toml`.
Dev: `tempfile`.

The `worktrunk` lib API is documented as unstable; we pin the minor version
(`worktrunk = "0.68"` — for 0.x crates cargo's caret already restricts to
patch-level updates) and accept upgrade churn in exchange for
`git::Repository` (subprocess wrapper with stderr-gutter error rendering) and
`styling` (output identical to built-ins).

**Verified against docs.rs (0.68):** `Repository::current()`,
`require_current_branch()`, `branch()` (with `upstream()` and push-remote
accessors), `run_command()`, and `run_command_check()` are all public — every
method the port uses exists in the published lib.

### CLI surface

```
wt-wip [OPTIONS]              # bare = push shorthand
wt-wip push [--stage <all|tracked|none>] [-m <msg>] [--format <text|json>]
wt-wip pull [--format <text|json>]
```

- Bare invocation accepts push's flags (`wt wip --stage tracked`) via clap's
  optional-subcommand pattern (`args_conflicts_with_subcommands` + flatten of
  push args at the top level, default dispatch to push).
- Current branch only.
- Long help carries the append-only contract and the staging table from the
  fork's `after_long_help`.
- Exit codes: 0 on success (including up-to-date), non-zero on all failures.
- No shell-completion subcommand in v1 (YAGNI; `worktrunk-sync` precedent exists
  if wanted later).

### Push behavior (port of fork `push.rs`)

1. `Repository::current()`; require a current branch.
2. Resolve remote: `branch.<name>.pushRemote` → `remote.pushDefault` →
   `branch.<name>.remote` → `origin`.
3. Stage per resolved mode: `add -A` / `add -u` / nothing.
4. Commit only when `diff --cached --quiet` reports a dirty index; message is
   the `-m` override or `wip @ <hostname> — <RFC3339 seconds>`.
5. Count commits ahead of upstream (`rev-list --count @{upstream}..HEAD`; when
   no upstream exists, count all commits so the first push reports > 0).
6. `push` (`-u` on first push). A non-fast-forward rejection surfaces as:
   *"Push to <remote>/<branch> was rejected — the remote has commits you don't
   have. Run `wt wip pull` first."* with git's stderr in the error gutter.

### Pull behavior (port of fork `pull.rs`)

1. `fetch <remote> <branch>` for the current branch.
2. `rev-list --count HEAD..<remote>/<branch>`; 0 → "Already up to date", exit 0.
3. `merge --ff-only <remote>/<branch>` — refuses (changing nothing) on
   divergence or would-be-overwritten uncommitted changes; error message names
   `wt wip push` / manual reconciliation, git stderr in the gutter.

### Config resolution

Effective stage mode, first hit wins:

1. `--stage` flag
2. `[wip] stage` in project config (`<repo>/.config/wt.toml`)
3. `[wip] stage` in worktrunk user config (same discovery order worktrunk
   documents: `$WORKTRUNK_CONFIG` / XDG / platform dir)
4. Built-in default `all`

Lookup is a generic `toml::Value` read of only the `[wip]` table — never a
typed deserialization of worktrunk's schema, so upstream config evolution
cannot break us. Worktrunk tolerates unknown tables, so `[wip]` in `wt.toml`
is safe. Missing or malformed values degrade to the default. If the published
lib exposes config-path discovery helpers we use them; otherwise `config.rs`
implements the documented path list.

### JSON output

`--format json` prints one object to **stdout** (human messages stay on
**stderr**, same split as the fork):

```json
{"branch": "...", "remote": "...", "committed": "<sha>|null",
 "outcome": "pushed|up-to-date", "commits_pushed": 0}
{"branch": "...", "remote": "...",
 "outcome": "fast-forwarded|up-to-date", "commits_pulled": 0}
```

## Shared contracts (T0 foundation task)

Written verbatim by the T0 scaffold task; parallel tasks import them.

```rust
// src/types.rs

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
```

```rust
// src/config.rs — signature contract

/// Resolve the effective stage mode: CLI flag → project `[wip] stage`
/// → user `[wip] stage` → StageMode::All.
pub fn resolve_stage_mode(flag: Option<StageMode>, repo: &worktrunk::git::Repository) -> StageMode;
```

```rust
// src/util.rs — signature contracts

/// `wip @ <hostname> — <RFC3339 seconds>` (ported from the fork).
pub fn wip_message() -> String;

/// Push remote for `branch`: pushRemote → pushDefault → branch remote → "origin".
pub fn resolve_remote(repo: &worktrunk::git::Repository, branch: &str) -> anyhow::Result<String>;
```

Contract assertions (inline in each relevant test file, under a
`// --- Contract assertions ---` separator):

```rust
// tests/integration.rs
// --- Contract assertions ---
let _: fn() -> String = wt_wip::util::wip_message;
assert_eq!(serde_json::to_string(&PushOutcome::UpToDate).unwrap(), "\"up-to-date\"");
assert_eq!(serde_json::to_string(&PullOutcome::FastForwarded).unwrap(), "\"fast-forwarded\"");
```

## Testing

Two-repo harness via `tempfile`: a bare "remote" plus one or two clones, no
network. Ported/adapted from the fork's integration suite:

- push creates wip commit and sets upstream on first push
- push with clean tree + pending commits pushes without an empty commit
- push with clean tree + no pending commits reports up-to-date, exit 0
- `--stage tracked` excludes untracked files; `--stage none` commits only
  pre-staged content
- `[wip] stage` config applies when no flag given; `--stage` beats config
- pull fast-forwards with correct commit count; dirty-but-non-conflicting tree
  survives untouched
- divergence fails safely in both directions with the documented hints,
  no local mutation, non-zero exit
- `--format json` schema on both subcommands
- guard test: build fails if a destructive git substring appears anywhere in
  `src/` — forbidden substrings are exactly the fork's list: `--force`
  (never force-push) and `reset` (broad on purpose: catches `reset --hard`
  and friends; use `merge --ff-only` instead)

## Repo bootstrap & CI

- `git init` on `main`; scaffold is the first commit; feature work on
  `feat/wt-wip`. Public GitHub repo via `gh repo create --public` at
  bootstrap (the session workflow requires a push remote from day one).
- CI (`ci.yaml`): `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo test` on stable, Linux + macOS.
- v1 distribution is local-only: `cargo install --path .`. crates.io
  publishing (and name reservation) is deferred — a later, separate decision.
- README mirrors `worktrunk-sync`: what/why, install, usage, staging table,
  safety contract.

## Success Criteria

1. `cargo install --path .` produces a `wt-wip` binary; with it on PATH,
   `wt wip` invokes it through worktrunk's external-subcommand dispatch.
2. On a repo with changes, `wt wip` (bare) stages per mode, creates a
   `wip @ <host> — <timestamp>` commit, and pushes; the first-ever push sets
   upstream.
3. `wt wip` with a clean tree and no unpushed commits exits 0 reporting
   "already up to date"; with a clean tree but unpushed commits, it pushes
   them without creating an empty commit.
4. `wt wip push --stage tracked` excludes untracked files; `--stage none`
   commits only pre-staged content; `[wip] stage = "tracked"` in
   `.config/wt.toml` applies when no flag is given, and `--stage` beats config.
5. `wt wip pull` fast-forwards and reports the commit count; a
   dirty-but-non-conflicting tree survives a pull untouched.
6. Divergence fails safely in both directions: push rejection names
   `wt wip pull`, pull rejection names `wt wip push`; neither mutates local
   state; exit codes are non-zero on every failure path.
7. `--format json` on both subcommands emits the documented schema on stdout
   with human output on stderr only.
8. The guard test fails the build if either forbidden substring (`--force`,
   `reset`) appears anywhere in `src/`.
9. `-m "custom"` overrides the generated wip message.
10. CI is green: fmt, clippy `-D warnings`, and the full test suite on
    Linux + macOS.
