// scriptum CLI entry point.

use clap::Parser;

mod client;
mod commands;
mod daemon_launcher;

#[derive(Parser)]
#[command(name = "scriptum", about = "Local-first collaborative markdown")]
struct Cli {
    #[command(subcommand)]
    command: commands::Command,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    daemon_launcher::ensure_daemon_running().await?;
    commands::run(cli.command)
}
