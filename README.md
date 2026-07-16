# worktrunk-wip

[![CI](https://github.com/bfirestone/worktrunk-wip/actions/workflows/ci.yaml/badge.svg)](https://github.com/bfirestone/worktrunk-wip/actions/workflows/ci.yaml)

`wt wip` — append-only, cross-machine WIP sync for [worktrunk](https://worktrunk.dev).

Stop half-finished work from being trapped on one machine. `wt wip` moves
in-progress changes between your machines through the git remote you already
have, with two commands and zero risk to history:

```bash
# desktop, 6pm
wt wip

# laptop, 8pm
wt wip pull   # …and keep working exactly where you left off
```

## The problem

You're mid-feature on your desktop and need to continue on your laptop. The
usual options are all bad in some way:

- **Hand-rolled `git add -A && git commit -m wip && git push`** — works until
  the day you `push --force` over the laptop's version, or `reset --hard` away
  something you needed.
- **`git stash` / patch files / scp** — invisible to the remote, easy to lose,
  doesn't survive machine failure.
- **"I'll just finish it here"** — no.

`wt wip` packages the safe version of the first option into a single command
with guardrails: it can only ever *add* commits on push and only ever
*fast-forward* on pull. There is no code path that rewrites or discards
history — and the test suite enforces that claim (see
[Safety guarantees](#safety-guarantees)).

## How it works

`wt wip` (push) does, in order:

1. **Stage** your changes — everything by default, configurable (see
   [Staging modes](#staging-modes)).
2. **Checkpoint** — create a commit *only if something is actually staged*,
   with an auto-generated message like:

   ```
   wip @ skynet — 2026-07-16T05:32:36Z
   ```

   The hostname tells you which machine made each checkpoint; the timestamp
   orders them. Clean tree with unpushed commits? It skips the commit and just
   pushes what's pending — no empty commits, ever.
3. **Push** — plain `git push`, setting the upstream automatically on a
   branch's first push. Never `--force`, never `--force-with-lease`.

`wt wip pull` does the mirror image:

1. **Fetch** the current branch from its remote.
2. **Fast-forward only** (`git merge --ff-only`). If your local branch has
   diverged, or the fast-forward would touch conflicting uncommitted changes,
   it refuses *and changes nothing* — your working tree and HEAD are exactly
   as they were.

Your branch history ends up as a stack of small wip checkpoints. That's
intentional: the checkpoints are disposable scaffolding. Squash them when the
work is ready (see [Cleaning up the wip stack](#cleaning-up-the-wip-stack)).

### Where `wt wip` comes from

The binary is `wt-wip`. Worktrunk dispatches any executable named `wt-<name>`
on your `PATH` as `wt <name>` — the same mechanism as git's `git-foo` plugins
(see [worktrunk's extending docs](https://worktrunk.dev/extending/#custom-subcommands)).
Everything also works by calling `wt-wip` directly; worktrunk just makes it
feel native.

## Requirements

- **git** ≥ 2.24 (uses `--end-of-options`)
- **Rust toolchain** to build (`cargo`)
- **worktrunk** — optional but recommended; only needed for the `wt wip`
  spelling and for `wt merge` squashing. `wt-wip` runs standalone without it.
- A git remote your machines can both reach (GitHub, GitLab, a bare repo on a
  server you SSH to — anything).

## Install

```bash
git clone https://github.com/bfirestone/worktrunk-wip
cd worktrunk-wip
cargo install --path .
```

This installs `wt-wip` into `~/.cargo/bin` (make sure that's on `PATH`).
Verify the worktrunk dispatch:

```bash
$ wt wip --help
Sync work across machines (append-only).
...
```

Repeat on each machine you want to sync between.

## Quick start: the two-machine loop

Machine A (desktop), mid-feature on branch `feat/parser`:

```bash
$ wt wip
◎ Committing wip checkpoint...
◎ Pushing feat/parser to origin...
✓ Pushed feat/parser to origin
```

Machine B (laptop), same repo, same branch checked out:

```bash
$ wt wip pull
◎ Fetching feat/parser from origin...
✓ Fast-forwarded feat/parser to origin/feat/parser (3 commits)
```

Work on the laptop, then reverse the direction:

```bash
laptop$  wt wip
desktop$ wt wip pull
```

That's the whole loop. Alternate freely; as long as you `pull` before you
start typing on a machine, the branch fast-forwards cleanly forever.

## Command reference

### `wt wip` / `wt wip push`

Bare `wt wip` is shorthand for `wt wip push` and accepts the same flags.

```
wt wip [--stage <all|tracked|none>] [-m <msg>] [--format <text|json>]
wt wip push [--stage <all|tracked|none>] [-m <msg>] [--format <text|json>]
```

| Flag | Effect |
|------|--------|
| `--stage <mode>` | What to stage before the checkpoint commit (see below). Beats config. |
| `-m, --message <msg>` | Replace the auto-generated `wip @ host — timestamp` message. |
| `--format json` | Print a machine-readable result object to stdout (see [Automation](#automation--json-output)). |

### `wt wip pull`

```
wt wip pull [--format <text|json>]
```

Both commands operate on the **current branch** of the repository you run
them in, and resolve the remote the same way git does for pushes
(`branch.<name>.pushRemote` → `remote.pushDefault` → `branch.<name>.remote`),
falling back to `origin`.

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success — including "already up to date" no-ops. |
| `1` | Runtime failure: push rejected, can't fast-forward, not a git repo, network error. Nothing was changed on failure. |
| `2` | CLI usage error (bad flag, conflicting args) — from clap. |

## Staging modes

| Value | Behavior |
|-----------|-------------------------------------------------------------------------|
| `all` | Stage everything: untracked files + unstaged tracked changes (default) |
| `tracked` | Stage tracked changes only (like `git add -u`) |
| `none` | Stage nothing; commit only what's already in the index |

`tracked` is the right default if your builds drop artifacts that
`.gitignore` doesn't cover. `none` turns `wt wip` into "push exactly what I
staged myself."

## Configuration

Set a persistent staging default in worktrunk's config files — no separate
dotfile:

```toml
# ~/.config/worktrunk/config.toml   (user-wide)
# <repo>/.config/wt.toml            (per-project, committable)
[wip]
stage = "tracked"
```

Precedence, first match wins:

1. `--stage` flag
2. `[wip] stage` in the **project** config (`<repo>/.config/wt.toml`)
3. `[wip] stage` in the **user** config (`$WORKTRUNK_CONFIG_PATH` →
   `$XDG_CONFIG_HOME/worktrunk/config.toml` → `~/.config/worktrunk/config.toml`)
4. Built-in default: `all`

`wt-wip` reads only the `[wip]` table, via a schema-agnostic TOML lookup —
worktrunk itself ignores unknown tables, so the two tools share config files
without version coupling. A missing or malformed value silently degrades to
the default rather than erroring.

## Automation & JSON output

`--format json` prints exactly one JSON object to **stdout**; all
human-readable progress stays on **stderr** (ANSI colors are stripped
automatically when the stream isn't a TTY). Safe to pipe.

```bash
$ wt wip push --format json
{"branch":"feat/parser","remote":"origin","committed":"a1b2c3…","outcome":"pushed","commits_pushed":2}

$ wt wip pull --format json
{"branch":"feat/parser","remote":"origin","outcome":"fast-forwarded","commits_pulled":2}
```

**Push result fields:**

| Field | Type | Meaning |
|-------|------|---------|
| `branch` | string | Branch that was pushed. |
| `remote` | string | Remote it was pushed to. |
| `committed` | string \| null | SHA of the checkpoint commit created this run; `null` if nothing was staged. |
| `outcome` | `"pushed"` \| `"up-to-date"` | Whether anything actually moved. |
| `commits_pushed` | number | Commits sent (0 when up to date). |

**Pull result fields:** `branch`, `remote`, `outcome`
(`"fast-forwarded"` \| `"up-to-date"`), `commits_pulled`.

Example: checkpoint from a cron job, but only alert when something moved:

```bash
out=$(wt wip push --format json) && \
  [ "$(jq -r .outcome <<<"$out")" = "pushed" ] && notify "WIP pushed"
```

## When things diverge

Divergence happens when both machines committed since they last synced. Both
commands detect it and fail **safely** — non-zero exit, nothing modified,
with an error that names the way out:

**Push rejected** (the remote has commits you don't have):

```
✗ Push to origin/feat/parser was rejected — the remote has commits you
  don't have. Run `wt wip pull` first.
```

→ Run `wt wip pull`. If pull then also refuses, you have true divergence:

**Pull refused** (local commits the remote doesn't have, or conflicting
uncommitted changes):

```
✗ Cannot fast-forward feat/parser to origin/feat/parser — the local branch
  has diverged (you may have unpushed commits) or has conflicting
  uncommitted changes. Run `wt wip push` or reconcile manually.
```

→ Reconcile once, manually, with full context — e.g.:

```bash
git pull --rebase        # replay your local wip commits on top of the remote
wt wip                   # back to the append-only loop
```

`wt wip` deliberately does **not** auto-rebase or auto-merge for you: a wrong
guess during automatic reconciliation is exactly the class of data loss this
tool exists to prevent. It gets you to the decision point safely and hands
over.

The habit that avoids divergence entirely: **`wt wip pull` when you sit
down, `wt wip` when you stand up.**

## Cleaning up the wip stack

Your branch will accumulate checkpoints like:

```
a1b2c3 wip @ laptop — 2026-07-16T08:14:02Z
d4e5f6 wip @ skynet — 2026-07-16T05:32:36Z
789abc wip @ skynet — 2026-07-15T22:10:11Z
```

They're scaffolding, not history. Before opening a PR, squash them on
whichever machine you finish on:

- **worktrunk:** `wt merge` squashes the branch when merging to trunk.
- **plain git:** `git rebase -i main` and squash/fixup the wip commits, or
  squash-merge the PR at the forge.

(Yes, that's a history rewrite — done once, deliberately, by you, at the
end. The invariant is that *`wt wip` itself* never rewrites anything.)

## Safety guarantees

- **Push is append-only.** No `--force` of any flavor exists in the codebase.
- **Pull is fast-forward-only.** No `reset`, no `checkout --`, no `clean`.
  The only mutation is `merge --ff-only`, which git guarantees refuses
  unless the move is a pure fast-forward.
- **Failures change nothing.** Every error path was tested to leave HEAD,
  the index, and the working tree untouched.
- **Enforced, not promised:** `tests/guard.rs` scans the entire `src/` tree
  and fails the build if a forbidden git invocation ever appears — in code,
  comments, or help text. CI runs it on every push.
- **Uncommitted work survives pulls.** A dirty-but-non-conflicting tree
  fast-forwards under you without being touched (covered by tests).

## Development

```
src/
├── main.rs       # clap CLI: push/pull + bare shorthand, JSON output
├── push.rs       # stage → checkpoint → append-only push
├── pull.rs       # fetch → ff-only merge
├── config.rs     # [wip] stage resolution from worktrunk config
├── types.rs      # StageMode, Push/PullResult (serde contracts)
├── util.rs       # wip message, remote resolution
└── testutil.rs   # shared test lock/harness (cfg(test) only)
tests/
├── guard.rs      # destructive-git source scan
└── integration.rs# 9 end-to-end tests driving the compiled binary
```

```bash
cargo test                                  # 28 tests: unit + guard + e2e
cargo fmt --all --check                     # enforced in CI
cargo clippy --all-targets -- -D warnings   # enforced in CI
```

The end-to-end tests build real bare-remote + clone fixtures in temp dirs —
no network, no mocks. CI runs the full gate on Linux and macOS.

Built on the published [`worktrunk`](https://crates.io/crates/worktrunk)
library crate (`git::Repository`, `styling`, config-path discovery), so
output and behavior match worktrunk's built-in commands. Related prior art:
[`worktrunk-sync`](https://github.com/pablospe/worktrunk-sync), which rebases
stacked worktree branches — different job, same extension mechanism.

## FAQ

**Why commits instead of `git stash`?** Stashes are local-only, invisible to
the remote, and don't survive a dead disk. Commits are replicated, ordered,
attributable to a machine, and get CI for free if you want it.

**Why not autosquash the wip commits on push?** Squashing rewrites history,
which would force-push, which breaks the other machine's fast-forward pull.
Append-only is what makes the loop safe in both directions.

**Does it work without worktrunk?** Yes — call `wt-wip` directly. Worktrunk
only provides the `wt wip` spelling and the `wt merge` cleanup convenience.

**What about branches with no remote counterpart yet?** First `wt wip` on a
branch pushes with `-u`, creating the remote branch and setting upstream
tracking in one step.

**Multiple worktrees?** Each worktree has its own current branch; `wt wip`
operates on whichever worktree you run it in. It pairs naturally with
worktrunk's worktree-per-branch workflow.

## License

MIT
