//! wt-wip — append-only cross-machine WIP sync for worktrunk.
//! CLI wiring lands in the CLI task; this stub keeps the crate compiling.

mod config;
mod pull;
mod push;
#[cfg(test)]
mod testutil;
mod types;
mod util;

fn main() {
    eprintln!("wt-wip: CLI not wired yet (see the CLI task)");
    std::process::exit(2);
}
