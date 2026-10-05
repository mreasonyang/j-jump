//! Equivalent entry point; all command behavior lives in the canonical CLI.
#[path = "../main.rs"]
mod cli;

fn main() {
    cli::main();
}
