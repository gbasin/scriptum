// scriptum CLI entry point.

use clap::Parser;

mod client;
mod commands;
mod daemon_launcher;
pub mod exit_code;
pub mod output;

#[derive(Parser)]
#[command(name = "scriptum", about = "Local-first collaborative markdown")]
struct Cli {
    #[command(subcommand)]
    command: commands::Command,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let should_boot_daemon =
        !matches!(&cli.command, commands::Command::Setup(_) | commands::Command::Init(_));
    if should_boot_daemon {
        daemon_launcher::ensure_daemon_running().await?;
    }
    commands::run(cli.command)
}
