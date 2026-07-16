# worktrunk-wip

`wt wip` — append-only cross-machine WIP sync for
[worktrunk](https://worktrunk.dev).

Move in-progress work between machines through the git remote with two
commands and zero risk to history:

- **`wt wip`** (or `wt wip push`) — stages your changes, makes a
  `wip @ <hostname> — <timestamp>` checkpoint commit, and pushes.
  Sets the upstream on the first push. Never force-pushes.
- **`wt wip pull`** — fetches and fast-forwards. If your local branch has
  diverged or a fast-forward would touch conflicting uncommitted changes,
  it refuses and changes nothing.

Squash the wip checkpoint stack later with `wt merge` (or an interactive
squash of your choice).

## Install

```bash
cargo install --path .
```

The binary is `wt-wip`. With it on `PATH`, worktrunk's
[external-subcommand dispatch](https://worktrunk.dev/extending/#custom-subcommands)
makes it available as `wt wip`.

## Usage

```bash
# machine A — checkpoint and push everything
wt wip

# machine B — catch up
wt wip pull

# tracked changes only, custom message
wt wip push --stage tracked -m "checkpoint: before refactor"

# automation
wt wip push --format json
```

### Staging

| Value     | Behavior                                                                |
|-----------|-------------------------------------------------------------------------|
| `all`     | Stage everything: untracked files + unstaged tracked changes (default)  |
| `tracked` | Stage tracked changes only (like `git add -u`)                          |
| `none`    | Stage nothing, commit only what's already in the index                  |

Set a persistent default in worktrunk's user config
(`~/.config/worktrunk/config.toml`) or project config (`.config/wt.toml`):

```toml
[wip]
stage = "tracked"
```

The `--stage` flag always beats config.

## Safety model

Append-only, enforced in CI: the test suite scans the source and fails the
build if any history-rewriting git invocation appears. Push only ever adds
commits; pull only ever fast-forwards. Divergence is reported with a hint
(`wt wip pull` / `wt wip push`) instead of being "resolved" destructively.

## License

MIT
