use clap::Args;

#[derive(Debug, Args)]
pub struct ClaimArgs {}

pub fn run(_args: ClaimArgs) -> anyhow::Result<()> {
    anyhow::bail!("'claim' command is not implemented yet")
}
