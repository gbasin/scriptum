use clap::Args;

#[derive(Debug, Args)]
pub struct LsArgs {}

pub fn run(_args: LsArgs) -> anyhow::Result<()> {
    anyhow::bail!("'ls' command is not implemented yet")
}
