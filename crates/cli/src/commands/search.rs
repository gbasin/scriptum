use clap::Args;

#[derive(Debug, Args)]
pub struct SearchArgs {}

pub fn run(_args: SearchArgs) -> anyhow::Result<()> {
    anyhow::bail!("'search' command is not implemented yet")
}
