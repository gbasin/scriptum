use clap::Args;

#[derive(Debug, Args)]
pub struct StatusArgs {}

pub fn run(_args: StatusArgs) -> anyhow::Result<()> {
    anyhow::bail!("'status' command is not implemented yet")
}
