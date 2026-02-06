use clap::Args;

#[derive(Debug, Args)]
pub struct SetupArgs {}

pub fn run(_args: SetupArgs) -> anyhow::Result<()> {
    anyhow::bail!("'setup' command is not implemented yet")
}
