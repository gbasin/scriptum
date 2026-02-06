use clap::Args;

#[derive(Debug, Args)]
pub struct DiffArgs {}

pub fn run(_args: DiffArgs) -> anyhow::Result<()> {
    anyhow::bail!("'diff' command is not implemented yet")
}
