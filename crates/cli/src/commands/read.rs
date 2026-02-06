use clap::Args;

#[derive(Debug, Args)]
pub struct ReadArgs {}

pub fn run(_args: ReadArgs) -> anyhow::Result<()> {
    anyhow::bail!("'read' command is not implemented yet")
}
