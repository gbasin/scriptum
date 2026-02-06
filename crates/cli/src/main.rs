// scriptum CLI entry point.

use clap::Parser;

mod commands;

#[derive(Parser)]
#[command(name = "scriptum", about = "Local-first collaborative markdown")]
struct Cli {
    #[command(subcommand)]
    command: commands::Command,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    commands::run(cli.command)
}
