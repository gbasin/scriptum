use clap::Args;

#[derive(Debug, Args)]
pub struct PeekArgs {}

pub fn run(_args: PeekArgs) -> anyhow::Result<()> {
    anyhow::bail!("'peek' command is not implemented yet")
}
