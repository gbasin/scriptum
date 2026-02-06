use clap::Args;

#[derive(Debug, Args)]
pub struct TreeArgs {}

pub fn run(_args: TreeArgs) -> anyhow::Result<()> {
    anyhow::bail!("'tree' command is not implemented yet")
}
