//! wt-wip — append-only cross-machine WIP sync for worktrunk.
//!
//! Installed on PATH as `wt-wip`, worktrunk's external-subcommand dispatch
//! makes it available as `wt wip`.

mod config;
mod pull;
mod push;
#[cfg(test)]
mod testutil;
mod types;
mod util;

use clap::{Args, Parser, Subcommand, ValueEnum};

use types::StageMode;

const STAGING_HELP: &str = r#"## Staging

Controls what to stage before the wip checkpoint commit:

| Value   | Behavior                                                          |
|---------|--------------------------------------------------------------------|
| all     | Stage everything: untracked files + unstaged tracked changes (default) |
| tracked | Stage tracked changes only (like `git add -u`)                    |
| none    | Stage nothing, commit only what's already in the index            |

Configure the default in worktrunk's user or project config:

```toml
[wip]
stage = "tracked"
```"#;

/// Sync work across machines (append-only).
///
/// Bare `wt-wip` is shorthand for `wt-wip push`: it stages changes, makes a
/// `wip @ <hostname> — <timestamp>` checkpoint commit, and pushes. `pull`
/// fetches and fast-forwards. Append-only: never rewrites history; pull is
/// fast-forward only. Squash the wip stack later with `wt merge`.
#[derive(Parser)]
#[command(name = "wt-wip", version, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Bare invocation: push flags apply directly (`wt-wip --stage tracked`)
    #[command(flatten)]
    push: PushArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Commit and push work to the remote (append-only)
    ///
    /// Stages changes, makes a `wip` checkpoint commit, and pushes. Never
    /// rewrites history.
    #[command(after_long_help = STAGING_HELP)]
    Push(PushArgs),

    /// Fetch and fast-forward the local branch from the remote
    ///
    /// Fast-forward only — fails safely if the local branch has diverged.
    Pull {
        /// Output format
        #[arg(long, value_enum, default_value_t = Format::Text, help_heading = "Automation")]
        format: Format,
    },
}

#[derive(Args)]
struct PushArgs {
    /// What to stage before the wip commit [default: all, or `[wip] stage` config]
    #[arg(long, value_enum)]
    stage: Option<StageMode>,

    /// Override the auto-generated wip commit message
    #[arg(short = 'm', long)]
    message: Option<String>,

    /// Output format
    ///
    /// JSON prints a structured result to stdout after the command completes.
    #[arg(long, value_enum, default_value_t = Format::Text, help_heading = "Automation")]
    format: Format,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn run_push(args: PushArgs) -> anyhow::Result<()> {
    let result = push::push(args.stage, args.message)?;
    if args.format == Format::Json {
        println!("{}", serde_json::to_string(&result)?);
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Push(args)) => run_push(args),
        Some(Command::Pull { format }) => pull::pull().and_then(|r| {
            if format == Format::Json {
                println!("{}", serde_json::to_string(&r)?);
            }
            Ok(())
        }),
        None => run_push(cli.push),
    };
    if let Err(err) = result {
        eprintln!("{}", worktrunk::styling::error_message(format!("{err:#}")));
        std::process::exit(1);
    }
}
