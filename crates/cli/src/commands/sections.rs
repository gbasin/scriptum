use clap::Args;

#[derive(Debug, Args)]
pub struct SectionsArgs {}

pub fn run(_args: SectionsArgs) -> anyhow::Result<()> {
    anyhow::bail!("'sections' command is not implemented yet")
}
