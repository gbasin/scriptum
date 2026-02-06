use clap::Args;

#[derive(Debug, Args)]
pub struct BlameArgs {}

pub fn run(_args: BlameArgs) -> anyhow::Result<()> {
    anyhow::bail!("'blame' command is not implemented yet")
}
